//! cargo-heft: find out what takes up space in a Cargo target directory,
//! and with `cargo heft disk`, in the Cargo and rustup homes.
//!
//! Sizes are on-disk (allocated blocks), every inode is counted once, and
//! directories are included, so the total matches `du -s`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs, io};

/// `println!` for a program whose reader may go away: `cargo heft | head`
/// closes the pipe early, and that is a normal way to end, not a panic.
macro_rules! say {
    ($($arg:tt)*) => {
        if let Err(e) = ::std::io::Write::write_fmt(&mut ::std::io::stdout(), format_args!("{}\n", format_args!($($arg)*))) {
            if e.kind() != ::std::io::ErrorKind::BrokenPipe {
                eprintln!("cargo-heft: cannot write to stdout: {e}");
                std::process::exit(1);
            }
            std::process::exit(0);
        }
    };
}

mod disk;

/// Version of the `--json` output, in both modes. Raise it when a key is
/// removed or renamed, or a value changes meaning; adding a key does not
/// need a new version.
const SCHEMA_VERSION: u32 = 1;

const USAGE: &str = "\
Usage: cargo heft [--target-dir <DIR>] [--top <N>] [--json]
       cargo heft --what-if [--profile <NAME>] [--only <IDS>] [--json] [-- <cargo build args>]
       cargo heft disk [--json]

  --target-dir <DIR>  directory to measure (default: the workspace's target dir)
  --top <N>           crates to list (default 15)
  --json              machine-readable output
  --what-if           build this workspace from scratch once per profile setting,
                      in a temporary directory, and compare the sizes
  --profile <NAME>    profile for --what-if (default: dev)
  --only <IDS>        settings --what-if builds, comma-separated (default: all):
                      configured, line-tables, no-debug, no-incremental,
                      line-tables-no-incremental
  disk [--json]       the Cargo home and the rustup home by part, and what
                      removing each part would cost (reads only)
  -V, --version       print the version";

const EXAMPLES: &str = "\
Examples:
  cargo heft                          measure this workspace's target directory
  cargo heft --target-dir target/debug --top 5
  cargo heft --json | jq .total_bytes
  cargo heft --what-if -- -p my-crate what each profile setting would save
  cargo heft --what-if --only configured,line-tables
  cargo heft disk                     what Rust keeps in ~/.cargo and ~/.rustup";

/// Profile settings `--what-if` tries: the id `--only` takes, the name shown,
/// and (key, value) pairs of `[profile.*]`.
type Scenario = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);
const SCENARIOS: &[Scenario] = &[
    ("configured", "as configured", &[]),
    (
        "line-tables",
        "debug = \"line-tables-only\"",
        &[("debug", "line-tables-only")],
    ),
    ("no-debug", "debug = false", &[("debug", "false")]),
    (
        "no-incremental",
        "incremental = false",
        &[("incremental", "false")],
    ),
    (
        "line-tables-no-incremental",
        "line-tables-only, no incremental",
        &[("debug", "line-tables-only"), ("incremental", "false")],
    ),
];

/// Ctrl-C reaches cargo and this process together. Remembering it, instead of
/// dying at once, lets `--what-if` remove its scratch directory first.
#[cfg(unix)]
mod interrupt {
    use std::sync::atomic::{AtomicBool, Ordering};

    static HIT: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_sigint(_: i32) {
        HIT.store(true, Ordering::SeqCst);
    }

    unsafe extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }

    pub fn catch() {
        // SIGINT is 2 on every Unix. The handler only stores a flag, which is
        // async-signal-safe, and child processes get the default back at exec.
        unsafe { signal(2, on_sigint) };
    }

    pub fn hit() -> bool {
        HIT.load(Ordering::SeqCst)
    }
}

#[cfg(not(unix))]
mod interrupt {
    pub fn catch() {}
    pub fn hit() -> bool {
        false
    }
}

fn is_hash(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What a path inside a target directory is. Handles both layouts:
///
/// - classic: `deps/`, `.fingerprint/`, `build/<pkg>-<hash>/{out,...}`
/// - build-dir v2 (Cargo 1.100): `build/<pkg>/<hash>/{out,fingerprint,run}`,
///   where `out` holds compiler output, or a build script's `OUT_DIR` in the
///   unit that also has `run/` (`script_unit`).
fn classify(rel: &Path, executable: bool, script_unit: bool) -> &'static str {
    let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let name = comps.last().copied().unwrap_or("");
    let has = |c: &str| comps.contains(&c);
    // Everything under doc/ is documentation, including rustdoc's source
    // pages, which mirror build paths such as build/<pkg>-<hash>/out.
    match comps.first().copied() {
        Some("doc") => return "doc",
        Some("package") => return "package",
        _ => {}
    }
    if has("incremental") {
        return "incremental";
    }
    let build = comps.iter().position(|c| *c == "build");
    let after = |n: usize| build.and_then(|i| comps.get(i + n)).copied();
    let v2 = after(2).is_some_and(is_hash);
    if has(".fingerprint") || (v2 && after(3) == Some("fingerprint")) {
        return "fingerprint";
    }
    if comps.iter().any(|c| c.ends_with(".dSYM")) {
        return "debuginfo (dSYM)";
    }
    let script_name = name.starts_with("build_script_") || name.starts_with("build-script-");
    if script_name {
        // The script's own dep-info sits next to it and is not its output.
        return if name.ends_with(".d") {
            "dep-info"
        } else {
            "build script executable"
        };
    }
    // Everything else a build script unit holds is what running it left
    // behind: `OUT_DIR`, plus its captured output (`output`, `stderr`,
    // `root-output`, `invoked.timestamp`), which the classic layout keeps
    // next to `out/` and v2 keeps in `run/`.
    let classic_unit = after(1).is_some_and(|c| strip_hash(c).is_some());
    if classic_unit || (v2 && (after(3) == Some("run") || script_unit)) {
        return "build script output";
    }
    match rel.extension().and_then(|e| e.to_str()) {
        Some("rlib") => "rlib",
        Some("a" | "lib") => "static library",
        Some("rmeta") => "rmeta",
        Some("d") => "dep-info",
        Some("o") => "object file",
        Some("dylib" | "so" | "dll") => "dylib",
        Some("dwo" | "dwp") => "debuginfo (split DWARF)",
        Some("exe") => "binary",
        None if executable => "binary",
        _ => "other",
    }
}

/// `name-<16 hex digits>` -> `name`.
fn strip_hash(s: &str) -> Option<&str> {
    let (name, hash) = s.rsplit_once('-')?;
    is_hash(hash).then_some(name)
}

/// The crate a path belongs to, if its name says so.
fn crate_of(rel: &Path) -> Option<String> {
    let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let norm = |s: &str| s.replace('-', "_");
    if let Some(i) = comps.iter().position(|c| *c == "incremental") {
        // incremental/<crate>-<base36 hash>/... A build script's cache is
        // named after the script, not its package, so it is not a crate.
        return comps
            .get(i + 1)
            .and_then(|c| c.rsplit_once('-'))
            .map(|(n, _)| match n {
                _ if n.starts_with("build_script_") => "(build scripts)".into(),
                _ => norm(n),
            });
    }
    if let Some(i) = comps.iter().position(|c| *c == "build")
        && comps.get(i + 2).is_some_and(|h| is_hash(h))
    {
        return Some(norm(comps[i + 1])); // build/<pkg>/<hash>/...
    }
    for (i, comp) in comps.iter().enumerate() {
        let (stem, ext) = comp.split_once('.').unwrap_or((comp, ""));
        if let Some(name) = strip_hash(stem) {
            let is_lib =
                i + 1 == comps.len() && matches!(ext, "rlib" | "rmeta" | "dylib" | "so" | "a");
            let name = if is_lib {
                name.strip_prefix("lib").unwrap_or(name)
            } else {
                name
            };
            return Some(norm(name));
        }
    }
    None
}

/// Reads a package's target out of the name of one of Cargo's fingerprint
/// files (`bin-rg`, `lib-ignore`, `dep-lib-ignore`, ...), as (target, package).
/// This is how a binary called `rg` is known to belong to `ripgrep`.
fn target_of(rel: &Path) -> Option<(String, String)> {
    let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let norm = |s: &str| s.replace('-', "_");
    let package = match comps.iter().position(|c| *c == ".fingerprint") {
        Some(i) => strip_hash(comps.get(i + 1)?)?, // .fingerprint/<pkg>-<hash>/
        None => {
            // build/<pkg>/<hash>/fingerprint/
            let i = comps.iter().position(|c| *c == "build")?;
            let v2 = is_hash(comps.get(i + 2)?) && comps.get(i + 3) == Some(&"fingerprint");
            v2.then(|| comps[i + 1])?
        }
    };
    let name = comps.last()?;
    let name = name.strip_suffix(".json").unwrap_or(name);
    let name = ["dep-", "output-"]
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .unwrap_or(name);
    let name = name.strip_prefix("test-").unwrap_or(name);
    let target = ["bin-", "lib-", "example-", "bench-"]
        .iter()
        .find_map(|p| name.strip_prefix(p))?;
    Some((norm(target), norm(package)))
}

#[cfg(unix)]
fn disk_usage(md: &fs::Metadata) -> (u64, (u64, u64), bool) {
    use std::os::unix::fs::MetadataExt;
    (
        md.blocks() * 512,
        (md.dev(), md.ino()),
        md.mode() & 0o111 != 0,
    )
}

#[cfg(not(unix))]
fn disk_usage(md: &fs::Metadata) -> (u64, (u64, u64), bool) {
    // ponytail: logical size and no hard-link detection off unix; use the
    // Windows file-id APIs if Windows support is ever asked for.
    (md.len(), (0, 0), false)
}

#[derive(Default)]
struct Report {
    total: u64,
    by_kind: BTreeMap<String, u64>,
    by_profile: BTreeMap<String, u64>,
    by_crate: BTreeMap<String, u64>,
    by_content: BTreeMap<String, u64>,
    /// Bytes of standalone `.rmeta` files whose `.rlib` holds the same metadata.
    duplicated_metadata: u64,
}

/// Logical bytes of metadata and debuginfo inside one file.
#[derive(Default)]
struct Parts {
    metadata: u64,
    debuginfo: u64,
}

fn object_parts<'d, R: object::ReadRef<'d>>(data: R, p: &mut Parts) {
    use object::{Object, ObjectSection, SectionKind};
    let Ok(file) = object::File::parse(data) else {
        return;
    };
    for s in file.sections() {
        let size = s.file_range().map_or(0, |(_, len)| len);
        let name = s.name().unwrap_or("");
        if name == ".rustc" {
            p.metadata += size;
        } else if matches!(s.kind(), SectionKind::Debug | SectionKind::DebugString)
            || [".debug", ".zdebug", "__debug"]
                .iter()
                .any(|d| name.starts_with(d))
        {
            p.debuginfo += size;
        }
    }
}

/// Looks inside an archive (`.rlib`, which adds metadata to object files, or
/// a `.a` static library) or a single object file, executable or dylib.
///
/// Reads through a cache, so only the headers and section tables it parses
/// come off the disk, not the whole file: a debug binary or rlib can be
/// hundreds of MB, and a target directory holds thousands of them.
fn parts<R: io::Read + io::Seek>(file: R, is_archive: bool) -> Parts {
    let cache = object::ReadCache::new(file);
    let mut p = Parts::default();
    match object::read::archive::ArchiveFile::parse(&cache) {
        Ok(archive) if is_archive => {
            for member in archive.members().flatten() {
                if member.name() == b"lib.rmeta" {
                    p.metadata += member.size();
                } else {
                    let (offset, size) = member.file_range();
                    object_parts(cache.range(offset, size), &mut p);
                }
            }
        }
        _ => object_parts(&cache, &mut p),
    }
    p
}

struct File {
    rel: PathBuf,
    bytes: u64,
    id: (u64, u64),
    executable: bool,
    symlink: bool,
}

/// Puts the path in an I/O error, which otherwise names only the OS error.
fn at(path: &Path) -> impl Fn(io::Error) -> io::Error + '_ {
    move |e| io::Error::new(e.kind(), format!("cannot read {}: {e}", path.display()))
}

fn walk(
    root: &Path,
    dir: &Path,
    files: &mut Vec<File>,
    dirs: &mut Vec<(PathBuf, u64)>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir).map_err(at(dir))? {
        let path = entry.map_err(at(dir))?.path();
        let md = fs::symlink_metadata(&path).map_err(at(&path))?;
        let (bytes, id, executable) = disk_usage(&md);
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if md.is_dir() {
            dirs.push((rel, bytes));
            walk(root, &path, files, dirs)?;
        } else {
            files.push(File {
                rel,
                bytes,
                id,
                executable,
                symlink: md.file_type().is_symlink(),
            });
        }
    }
    Ok(())
}

/// The directories that have the shape of a profile directory: `debug`,
/// `release`, or `<triple>/debug`, but not `doc`, `package` or `tmp`.
fn profile_dirs(dirs: &[(PathBuf, u64)]) -> HashSet<PathBuf> {
    let all: HashSet<&Path> = dirs.iter().map(|(d, _)| d.as_path()).collect();
    let has = |d: &Path, child: &str| all.contains(d.join(child).as_path());
    all.iter()
        .copied()
        .filter(|d| {
            has(d, "build")
                && ["deps", "incremental", "examples"]
                    .iter()
                    .any(|c| has(d, c))
        })
        .map(Path::to_path_buf)
        .collect()
}

/// The same shape, checked on disk.
fn is_profile_dir(dir: &Path) -> bool {
    dir.join("build").is_dir()
        && ["deps", "incremental", "examples"]
            .iter()
            .any(|c| dir.join(c).is_dir())
}

/// When `--target-dir` points inside a target directory (`target/debug`,
/// `target/doc`), where it sits in that directory: (target root, path of the
/// measured directory relative to it). Paths are then read as if measured
/// from the root, so a profile, `doc/` or a build script unit keeps its
/// meaning. Cargo marks a target root with CACHEDIR.TAG; the first component
/// must be one Cargo creates there, so an unrelated directory that happens
/// to live under a target (`target/tmp/...`) is measured as it is.
fn locate_in_target(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let root = root.canonicalize().ok()?;
    if root.join("CACHEDIR.TAG").is_file() {
        return None;
    }
    let base = root
        .ancestors()
        .skip(1)
        .take(4)
        .find(|a| a.join("CACHEDIR.TAG").is_file())?;
    let prefix = root.strip_prefix(base).ok()?.to_path_buf();
    let first = base.join(prefix.iter().next()?);
    let known = ["doc", "package"].iter().any(|d| first.ends_with(d))
        || is_profile_dir(&first)
        || fs::read_dir(&first)
            .ok()?
            .flatten()
            .any(|e| is_profile_dir(&e.path())); // <triple>/<profile>
    known.then(|| (base.to_path_buf(), prefix))
}

/// The outermost profile directory a path is in.
fn profile_of(rel: &Path, profiles: &HashSet<PathBuf>) -> String {
    let mut inside: Vec<&Path> = rel.ancestors().filter(|a| profiles.contains(*a)).collect();
    match inside.pop() {
        Some(profile) => profile.to_string_lossy().into_owned(),
        None => "(not in a profile)".into(),
    }
}

/// What the whole tree says about how to read a single path.
struct Layout {
    /// Directories that are profiles.
    profiles: HashSet<PathBuf>,
    /// Target name to package name, where Cargo's fingerprints tell.
    packages: HashMap<String, String>,
}

impl Report {
    fn add(&mut self, rel: &Path, kind: &str, bytes: u64, layout: &Layout) {
        self.total += bytes;
        *self.by_kind.entry(kind.into()).or_default() += bytes;
        *self
            .by_profile
            .entry(profile_of(rel, &layout.profiles))
            .or_default() += bytes;
        if let Some(c) = crate_of(rel) {
            // Files are named after the target; list them under its package.
            let c = layout.packages.get(&c).cloned().unwrap_or(c);
            *self.by_crate.entry(c).or_default() += bytes;
        }
    }
}

impl Report {
    /// Splits a file's on-disk bytes by what is inside it.
    fn add_content(
        &mut self,
        root: &Path,
        f: &File,
        kind: &str,
        rlibs: &mut BTreeMap<PathBuf, u64>,
    ) {
        let mut put = |name: &str, bytes: u64| {
            if bytes > 0 {
                *self.by_content.entry(name.into()).or_default() += bytes;
            }
        };
        match kind {
            "rlib"
            | "static library"
            | "object file"
            | "binary"
            | "dylib"
            | "build script executable" => {
                // A file that cannot be opened counts as code, as before.
                let (len, p) = match fs::File::open(root.join(&f.rel)) {
                    Ok(file) => (
                        file.metadata().map_or(0, |m| m.len()),
                        parts(file, matches!(kind, "rlib" | "static library")),
                    ),
                    Err(_) => (0, Parts::default()),
                };
                // Scale logical bytes to the blocks the file occupies.
                let scale = |n: u64| (n as u128 * f.bytes as u128 / len.max(1) as u128) as u64;
                let (metadata, debuginfo) = (scale(p.metadata).min(f.bytes), scale(p.debuginfo));
                let debuginfo = debuginfo.min(f.bytes - metadata);
                put("metadata (inside rlib/dylib)", metadata);
                put("debuginfo", debuginfo);
                put("code, data and symbols", f.bytes - metadata - debuginfo);
                if kind == "rlib" {
                    rlibs.insert(f.rel.with_extension(""), metadata);
                }
            }
            "rmeta" => put("metadata (.rmeta files)", f.bytes),
            "debuginfo (dSYM)" | "debuginfo (split DWARF)" => put("debuginfo", f.bytes),
            "incremental" => put("incremental cache", f.bytes),
            _ => put("other", f.bytes),
        }
    }
}

fn measure(root: &Path) -> io::Result<Report> {
    let mut r = Report::default();
    let mut files = Vec::new();
    let (bytes, ..) = disk_usage(&fs::symlink_metadata(root).map_err(at(root))?);
    // Inside a target directory, name paths from its root (see locate_in_target).
    let (base, prefix) = locate_in_target(root).unwrap_or((root.to_path_buf(), PathBuf::new()));
    let mut dirs = vec![(prefix.clone(), bytes)];
    walk(&base, root, &mut files, &mut dirs)?;
    let mut profiles = profile_dirs(&dirs);
    // A profile the measured directory sits in, such as `debug` when measuring
    // `debug/build`, is above it and was not walked.
    profiles.extend(
        prefix
            .ancestors()
            .filter(|a| !a.as_os_str().is_empty() && is_profile_dir(&base.join(a)))
            .map(Path::to_path_buf),
    );
    let root = base.as_path();
    let layout = Layout {
        profiles,
        packages: files.iter().filter_map(|f| target_of(&f.rel)).collect(),
    };
    // A v2 unit that has `run/` is a build script run; its `out/` is OUT_DIR.
    let script_units: HashSet<&Path> = dirs
        .iter()
        .filter(|(d, _)| d.file_name().is_some_and(|n| n == "run"))
        .filter_map(|(d, _)| d.parent())
        .collect();
    for (dir, bytes) in &dirs {
        r.add(dir, "directories", *bytes, &layout);
    }
    // rustc hard-links objects and metadata between `deps` and `incremental`.
    // Count each inode once, as the artifact outside `incremental`, so that
    // "incremental" is what deleting that directory would actually free.
    let in_incremental = |f: &File| f.rel.iter().any(|c| c == "incremental");
    files.sort_by(|a, b| (in_incremental(a), &a.rel).cmp(&(in_incremental(b), &b.rel)));
    let mut seen = HashSet::new();
    let (mut rlibs, mut rmetas) = (BTreeMap::new(), Vec::new());
    for f in &files {
        if cfg!(unix) && !seen.insert(f.id) {
            continue;
        }
        let script_unit = f.rel.ancestors().any(|a| script_units.contains(a));
        // A symlink is not what its name says: it is a link, with mode 0777,
        // and what it points to may be outside the target directory.
        let kind = match f.symlink {
            true => "symlink",
            false => classify(&f.rel, f.executable, script_unit),
        };
        r.add(&f.rel, kind, f.bytes, &layout);
        r.add_content(root, f, kind, &mut rlibs);
        if kind == "rmeta" {
            rmetas.push(f);
        }
    }
    r.duplicated_metadata = rmetas
        .iter()
        .filter_map(|f| {
            rlibs
                .get(&f.rel.with_extension(""))
                .map(|inside| f.bytes.min(*inside))
        })
        .sum();
    let dirs = r.by_kind.get("directories").copied().unwrap_or(0);
    if dirs > 0 {
        *r.by_content.entry("other".into()).or_default() += dirs;
    }
    Ok(r)
}

fn cargo() -> Command {
    Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Removes the scratch directory however `what_if` ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stop_if_interrupted(scratch: &Scratch) {
    if interrupt::hit() {
        let _ = fs::remove_dir_all(&scratch.0);
        eprintln!("interrupted; the temporary build directory was removed");
        std::process::exit(130); // 128 + SIGINT, as a shell reports it
    }
}

fn what_if(
    profile: &str,
    only: &[&Scenario],
    extra: &[OsString],
    json: bool,
) -> Result<(), String> {
    interrupt::catch();
    let scratch = Scratch(env::temp_dir().join(format!("cargo-heft-{}", std::process::id())));
    let _ = cargo().arg("fetch").output(); // keep downloads out of the first timing
    let prefix = format!(
        "CARGO_PROFILE_{}_",
        profile.to_uppercase().replace('-', "_")
    );
    let mut rows = Vec::new();
    for (i, &&(id, name, settings)) in only.iter().enumerate() {
        stop_if_interrupted(&scratch);
        eprintln!("building {}/{}: {name}", i + 1, only.len());
        let dir = scratch.0.join(i.to_string());
        let start = std::time::Instant::now();
        let out = cargo()
            .args(["build", "--profile", profile])
            .args(extra)
            .env("CARGO_TARGET_DIR", &dir)
            .envs(
                settings
                    .iter()
                    .map(|(k, v)| (format!("{prefix}{}", k.to_uppercase()), v)),
            )
            .output()
            .map_err(|e| format!("could not run cargo build: {e}"))?;
        let seconds = start.elapsed().as_secs_f64();
        stop_if_interrupted(&scratch);
        if !out.status.success() {
            return Err(format!(
                "cargo build failed ({name}):\n{}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let total = measure(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .total;
        let _ = fs::remove_dir_all(&dir); // one build on disk at a time
        rows.push((id, name, settings, total, seconds));
    }
    drop(scratch); // before printing: a closed pipe ends the process at once
    if json {
        let scenarios: Vec<_> = rows
            .iter()
            .map(|(id, name, settings, total, seconds)| {
                let settings: BTreeMap<_, _> = settings.iter().copied().collect();
                serde_json::json!({ "id": id, "name": name, "settings": settings, "total_bytes": total, "build_seconds": seconds })
            })
            .collect();
        say!(
            "{:#}",
            serde_json::json!({ "schema_version": SCHEMA_VERSION, "profile": profile, "scenarios": scenarios })
        );
        return Ok(());
    }
    let base = rows[0].3;
    say!("fresh `{profile}` builds, settings as they would go in [profile.{profile}]\n");
    say!(
        "  {:<34} {:>10} {:>9} {:>8}",
        "setting",
        "size",
        "vs first",
        "build"
    );
    for (_, name, _, total, seconds) in &rows {
        let delta = if *total == base || base == 0 {
            String::new()
        } else {
            format!(
                "{:+.1}%",
                100.0 * (*total as f64 - base as f64) / base as f64
            )
        };
        say!(
            "  {name:<34} {:>10} {delta:>9} {seconds:>7.1}s",
            human(*total)
        );
    }
    say!("\n  note: these are builds from scratch. `incremental = false` makes rebuilds after an");
    say!("  edit slower, so it suits CI more than local work. Less debuginfo means a debugger");
    say!("  shows less: `line-tables-only` keeps backtraces with file and line.");
    Ok(())
}

fn default_target_dir() -> Result<PathBuf, String> {
    if let Some(d) = env::var_os("CARGO_TARGET_DIR") {
        return Ok(d.into());
    }
    let out = cargo()
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    v["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "no target_directory".into())
}

/// Decimal units (1 kB = 1000 bytes). The unit is picked after rounding, so
/// 999 999 bytes is "1.0 MB" and not "1000.0 kB".
fn human(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1e3;
    if kb < 999.95 {
        format!("{kb:.1} kB")
    } else if kb / 1e3 < 999.95 {
        format!("{:.1} MB", kb / 1e3)
    } else {
        format!("{:.2} GB", kb / 1e6)
    }
}

fn sorted(m: &BTreeMap<String, u64>) -> Vec<(&String, &u64)> {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v
}

fn print_table(title: &str, m: &BTreeMap<String, u64>, total: u64, top: usize) {
    say!("\n{title}");
    for (name, bytes) in sorted(m).into_iter().take(top) {
        let pct = if total == 0 {
            0.0
        } else {
            100.0 * *bytes as f64 / total as f64
        };
        say!("  {name:<28} {:>12} {pct:>6.1}%", human(*bytes));
    }
}

/// The value of `flag`: the part after `=`, or the next argument.
fn value(
    flag: &str,
    inline: &mut Option<OsString>,
    args: &mut impl Iterator<Item = OsString>,
) -> Result<OsString, String> {
    inline
        .take()
        .or_else(|| args.next())
        .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
}

fn run() -> Result<(), String> {
    // `args_os`: paths need not be UTF-8, and `env::args` panics on those.
    let mut args = env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|a| a == "heft") {
        args.next(); // invoked as `cargo heft`
    }
    if args.peek().is_some_and(|a| a == "disk") {
        args.next();
        return disk::run(args);
    }
    let (mut target, mut top, mut json) = (None, None, false);
    let (mut whatif, mut profile, mut extra, mut only) = (false, None, None, None);
    while let Some(a) = args.next() {
        // `--flag=value` as well as `--flag value`.
        let (flag, mut inline) = match a.to_str().and_then(|s| s.split_once('=')) {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_string(), Some(OsString::from(value)))
            }
            _ => (a.to_string_lossy().into_owned(), None),
        };
        match flag.as_str() {
            "--target-dir" => target = Some(PathBuf::from(value(&flag, &mut inline, &mut args)?)),
            "--top" => {
                let n = value(&flag, &mut inline, &mut args)?;
                top = Some(n.to_str().and_then(|n| n.parse().ok()).ok_or_else(|| {
                    format!(
                        "--top needs a whole number, got `{}`\n{USAGE}",
                        n.to_string_lossy()
                    )
                })?);
            }
            "--profile" => {
                let name = value(&flag, &mut inline, &mut args)?;
                profile = Some(
                    name.into_string()
                        .map_err(|_| "--profile needs a profile name in UTF-8")?,
                );
            }
            "--only" => {
                let ids = value(&flag, &mut inline, &mut args)?;
                let ids = ids.to_string_lossy();
                let mut picked = Vec::new();
                for id in ids.split(',').map(str::trim) {
                    let s = SCENARIOS.iter().find(|s| s.0 == id).ok_or_else(|| {
                        let known: Vec<_> = SCENARIOS.iter().map(|s| s.0).collect();
                        format!(
                            "--only: unknown setting `{id}`, expected some of: {}",
                            known.join(", ")
                        )
                    })?;
                    if !picked.contains(&s) {
                        picked.push(s);
                    }
                }
                only = Some(picked);
            }
            "--json" => json = true,
            "--what-if" => whatif = true,
            "--" => extra = Some(args.by_ref().collect::<Vec<_>>()),
            "-V" | "--version" => {
                say!("cargo-heft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                say!("{}\n\n{USAGE}\n\n{EXAMPLES}", env!("CARGO_PKG_DESCRIPTION"));
                return Ok(());
            }
            _ => return Err(format!("unknown argument `{flag}`\n{USAGE}")),
        }
        if inline.is_some() {
            return Err(format!("{flag} takes no value\n{USAGE}"));
        }
    }
    // A flag that would be ignored is a mistake worth pointing out.
    let misplaced = match whatif {
        true if target.is_some() => {
            Some("--target-dir does not apply to --what-if, which builds in a temporary directory")
        }
        true if top.is_some() => Some("--top does not apply to --what-if"),
        false if profile.is_some() => Some("--profile only applies to --what-if"),
        false if only.is_some() => Some("--only applies to --what-if"),
        false if extra.is_some() => {
            Some("arguments after `--` go to `cargo build`, so they only apply to --what-if")
        }
        _ => None,
    };
    if let Some(why) = misplaced {
        return Err(format!("{why}\n{USAGE}"));
    }
    let top = top.unwrap_or(15usize);
    if whatif {
        let all: Vec<_> = SCENARIOS.iter().collect();
        return what_if(
            profile.as_deref().unwrap_or("dev"),
            &only.unwrap_or(all),
            &extra.unwrap_or_default(),
            json,
        );
    }
    let target = match target {
        Some(t) => t,
        None => default_target_dir()?,
    };
    if !target.is_dir() {
        let why = if target.exists() {
            "is not a directory"
        } else {
            "does not exist"
        };
        // A missing profile directory next to others: build that profile.
        let in_target = target
            .parent()
            .is_some_and(|p| p.join("CACHEDIR.TAG").is_file());
        let build = match target.file_name().and_then(|n| n.to_str()) {
            Some("release") if in_target => "cargo build --release".to_string(),
            Some(name) if in_target && name != "debug" => format!("cargo build --profile {name}"),
            _ => "cargo build".to_string(),
        };
        return Err(format!(
            "nothing to measure: {} {why}\nBuild the workspace first (`{build}`), or point at a target directory with --target-dir.",
            target.display()
        ));
    }
    // A symlinked target directory is measured where it points, as `du -sL`
    // would; `du -s` on the link counts only the link.
    let real = target.canonicalize().unwrap_or_else(|_| target.clone());
    let r = measure(&real).map_err(|e| {
        format!("{e}\nNothing was measured: a total that skipped it would be too small.")
    })?;
    if json {
        let out = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "target_dir": target.to_string_lossy(),
            "total_bytes": r.total,
            "by_kind": r.by_kind,
            "by_content": r.by_content,
            "duplicated_metadata_bytes": r.duplicated_metadata,
            "by_profile": r.by_profile,
            "by_crate": r.by_crate,
        });
        say!("{out:#}");
    } else {
        // du -h counts in units of 1024; say so before the totals look wrong.
        let shown = match fs::symlink_metadata(&target).is_ok_and(|m| m.is_symlink()) {
            true => format!("{} -> {}", target.display(), real.display()),
            false => target.display().to_string(),
        };
        say!(
            "{}: {} on disk ({:.1} MiB in the units du -h uses)",
            shown,
            human(r.total),
            r.total as f64 / 1_048_576.0
        );
        print_table("by kind", &r.by_kind, r.total, usize::MAX);
        print_table("by content", &r.by_content, r.total, usize::MAX);
        // "other" in this view is every kind that holds no compiled code.
        let compiled = [
            "rlib",
            "static library",
            "object file",
            "binary",
            "dylib",
            "build script executable",
            "rmeta",
            "debuginfo (dSYM)",
            "debuginfo (split DWARF)",
            "incremental",
        ];
        let mut rest: Vec<_> = r
            .by_kind
            .iter()
            .filter(|(k, b)| **b > 0 && !compiled.contains(&k.as_str()))
            .collect();
        rest.sort_by(|a, b| b.1.cmp(a.1));
        if !rest.is_empty() {
            let names: Vec<&str> = rest
                .iter()
                .map(|(k, _)| match k.as_str() {
                    "other" => "unclassified files",
                    k => k,
                })
                .collect();
            say!("  other = {}", names.join(", "));
        }
        if r.duplicated_metadata > 0 {
            say!(
                "  note: {} of metadata is stored twice, as a .rmeta file and inside its .rlib",
                human(r.duplicated_metadata)
            );
        }
        print_table("by profile", &r.by_profile, r.total, usize::MAX);
        print_table("largest crates", &r.by_crate, r.total, top);
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cargo-heft: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_name() {
        let k = |p: &str| classify(Path::new(p), false, false);
        assert_eq!(k("debug/deps/libserde-0123456789abcdef.rlib"), "rlib");
        assert_eq!(k("debug/deps/libserde-0123456789abcdef.rmeta"), "rmeta");
        assert_eq!(k("debug/deps/rg-0123456789abcdef.1x.rcgu.o"), "object file");
        assert_eq!(
            k("debug/incremental/rg-1abc/s-x/query-cache.bin"),
            "incremental"
        );
        assert_eq!(
            k("debug/build/libc-0123456789abcdef/out/gen.rs"),
            "build script output"
        );
        assert_eq!(
            k("debug/build/libc-0123456789abcdef/build_script_build-0123456789abcdef"),
            "build script executable"
        );
        assert_eq!(
            k("debug/deps/rg-0123456789abcdef.dSYM/Contents/x"),
            "debuginfo (dSYM)"
        );
        assert_eq!(classify(Path::new("debug/rg"), true, false), "binary");
    }

    #[test]
    fn classifies_the_v2_layout() {
        let k = |p: &str, script| classify(Path::new(p), false, script);
        let unit = "debug/build/dep/0123456789abcdef";
        assert_eq!(
            k(&format!("{unit}/out/libdep-0123456789abcdef.rlib"), false),
            "rlib"
        );
        assert_eq!(
            k(&format!("{unit}/out/libdep-0123456789abcdef.rmeta"), false),
            "rmeta"
        );
        assert_eq!(
            k(&format!("{unit}/fingerprint/lib-dep.json"), false),
            "fingerprint"
        );
        assert_eq!(
            k(&format!("{unit}/out/build_script_build"), false),
            "build script executable"
        );
        assert_eq!(
            k(&format!("{unit}/out/generated.rs"), true),
            "build script output"
        );
        assert_eq!(
            k(&format!("{unit}/run/stdout"), false),
            "build script output"
        );
        assert_eq!(
            classify(Path::new(&format!("{unit}/out/app")), true, false),
            "binary"
        );
        assert_eq!(
            crate_of(Path::new(&format!("{unit}/out/app"))).as_deref(),
            Some("dep")
        );
    }

    #[test]
    fn human_picks_the_unit_after_rounding() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(999), "999 B");
        assert_eq!(human(1000), "1.0 kB");
        assert_eq!(human(999_949), "999.9 kB");
        assert_eq!(human(999_999), "1.0 MB");
        assert_eq!(human(313_800_000), "313.8 MB");
        assert_eq!(human(999_999_999), "1.00 GB");
        assert_eq!(human(47_500_000_000), "47.50 GB");
    }

    #[test]
    fn build_script_runs_classify_alike_in_both_layouts() {
        let k = |p: &str, script| classify(Path::new(p), false, script);
        let classic = "debug/build/dep-0123456789abcdef";
        let v2 = "debug/build/dep/0123456789abcdef";
        for file in [
            "output",
            "root-output",
            "stderr",
            "invoked.timestamp",
            "out/generated.rs",
        ] {
            assert_eq!(
                k(&format!("{classic}/{file}"), false),
                "build script output",
                "{file}"
            );
        }
        for file in [
            "run/stdout",
            "run/root-output",
            "run/stderr",
            "run/invoked.timestamp",
        ] {
            assert_eq!(
                k(&format!("{v2}/{file}"), false),
                "build script output",
                "{file}"
            );
        }
        assert_eq!(
            k(&format!("{v2}/out/generated.rs"), true),
            "build script output"
        );
        // The script itself is not its output, in either layout.
        for exe in ["build_script_build-0123456789abcdef", "build-script-build"] {
            assert_eq!(
                k(&format!("{classic}/{exe}"), false),
                "build script executable"
            );
        }
        assert_eq!(
            k(&format!("{v2}/out/build_script_build"), false),
            "build script executable"
        );
    }

    #[test]
    fn only_profile_shaped_directories_are_profiles() {
        let dirs: Vec<(PathBuf, u64)> = [
            "",
            "debug",
            "debug/build",
            "debug/deps",
            "release",
            "release/build",
            "release/incremental",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu/debug",
            "x86_64-unknown-linux-gnu/debug/build",
            "x86_64-unknown-linux-gnu/debug/examples",
            "doc",
            "doc/serde",
            "package",
            "debug/build/dep-0123456789abcdef/out/build",
            "debug/build/dep-0123456789abcdef/out/deps",
        ]
        .iter()
        .map(|d| (PathBuf::from(d), 0))
        .collect();
        let profiles = profile_dirs(&dirs);
        let p = |rel: &str| profile_of(Path::new(rel), &profiles);
        assert_eq!(p("debug/deps/libserde-0123456789abcdef.rlib"), "debug");
        assert_eq!(p("release/rg"), "release");
        assert_eq!(
            p("x86_64-unknown-linux-gnu/debug/rg"),
            "x86_64-unknown-linux-gnu/debug"
        );
        // Not misled by a build script that makes its own `build` and `deps`.
        assert_eq!(p("debug/build/dep-0123456789abcdef/out/build/x.o"), "debug");
        assert_eq!(p("doc/serde/index.html"), "(not in a profile)");
        assert_eq!(p("package/rg-1.0.0.crate"), "(not in a profile)");
        assert_eq!(p("CACHEDIR.TAG"), "(not in a profile)");
    }

    #[test]
    fn fingerprints_name_the_package_of_a_target() {
        let t = |p: &str| target_of(Path::new(p));
        let pair = |target: &str, package: &str| Some((target.to_string(), package.to_string()));
        let classic = "debug/.fingerprint/ripgrep-0123456789abcdef";
        assert_eq!(t(&format!("{classic}/bin-rg")), pair("rg", "ripgrep"));
        assert_eq!(t(&format!("{classic}/bin-rg.json")), pair("rg", "ripgrep"));
        assert_eq!(t(&format!("{classic}/dep-bin-rg")), pair("rg", "ripgrep"));
        assert_eq!(t(&format!("{classic}/test-bin-rg")), pair("rg", "ripgrep"));
        assert_eq!(t(&format!("{classic}/invoked.timestamp")), None);
        let v2 = "debug/build/proc-macro2/0123456789abcdef/fingerprint";
        assert_eq!(
            t(&format!("{v2}/lib-proc_macro2.json")),
            pair("proc_macro2", "proc_macro2")
        );
        assert_eq!(
            t(&format!("{v2}/output-lib-proc_macro2")),
            pair("proc_macro2", "proc_macro2")
        );
        assert_eq!(
            t(&format!("{v2}/run-build-script-build-script-build")),
            None
        );
        assert_eq!(t("debug/deps/rg-0123456789abcdef"), None);
    }

    #[test]
    fn doc_and_dep_info_are_not_mistaken_for_build_script_files() {
        let k = |p: &str| classify(Path::new(p), false, false);
        // rustdoc's source page for a file a build script generated.
        assert_eq!(
            k("doc/src/dep/abs/target/debug/build/dep-0123456789abcdef/out/generated.rs.html"),
            "doc"
        );
        assert_eq!(classify(Path::new("doc/.lock"), true, false), "doc");
        assert_eq!(
            k("debug/build/dep-0123456789abcdef/build_script_build-0123456789abcdef.d"),
            "dep-info"
        );
        assert_eq!(
            k("debug/build/dep-0123456789abcdef/build_script_build-0123456789abcdef"),
            "build script executable"
        );
    }

    #[test]
    fn names_the_crate() {
        let c = |p: &str| crate_of(Path::new(p));
        assert_eq!(
            c("debug/deps/liblibc-0123456789abcdef.rlib").as_deref(),
            Some("libc")
        );
        assert_eq!(
            c("debug/deps/regex_syntax-0123456789abcdef.d").as_deref(),
            Some("regex_syntax")
        );
        assert_eq!(
            c("debug/build/serde-json-0123456789abcdef/build_script_build-0123456789abcdef")
                .as_deref(),
            Some("serde_json")
        );
        assert_eq!(
            c("debug/incremental/grep_cli-2k1x9/s-a-b/dep-graph.bin").as_deref(),
            Some("grep_cli")
        );
        assert_eq!(c("debug/rg"), None);
        assert_eq!(
            c("debug/incremental/build_script_build-2k1x9/s-a-b/dep-graph.bin").as_deref(),
            Some("(build scripts)")
        );
    }

    #[test]
    fn hashes_are_exactly_sixteen_hex_digits() {
        assert!(is_hash("0123456789abcdef"));
        assert!(is_hash("ABCDEF0123456789"));
        assert!(!is_hash(""));
        assert!(!is_hash("0123456789abcde"));
        assert!(!is_hash("0123456789abcdef0"));
        assert!(!is_hash("0123456789abcdeg"));
        assert!(!is_hash("0123456789abcde-"));
    }

    #[test]
    fn strip_hash_keeps_everything_before_the_last_dash() {
        assert_eq!(strip_hash("foo-0123456789abcdef"), Some("foo"));
        assert_eq!(strip_hash("foo-bar-0123456789abcdef"), Some("foo-bar"));
        assert_eq!(
            strip_hash("libserde_json-0123456789abcdef"),
            Some("libserde_json")
        );
        assert_eq!(strip_hash("foo"), None);
        assert_eq!(strip_hash("foo-0123456789abcde"), None);
        assert_eq!(strip_hash("0123456789abcdef"), None);
        assert_eq!(strip_hash("-0123456789abcdef"), Some(""));
    }

    #[test]
    fn classifies_every_known_artifact_kind() {
        let k = |p: &str| classify(Path::new(p), false, false);
        let cases = [
            ("release/deps/libserde-0123456789abcdef.rlib", "rlib"),
            ("release/deps/libserde-0123456789abcdef.rmeta", "rmeta"),
            ("release/deps/app-0123456789abcdef.d", "dep-info"),
            ("release/deps/app-0123456789abcdef.1x.rcgu.o", "object file"),
            ("release/deps/libfoo-0123456789abcdef.dylib", "dylib"),
            ("release/deps/libfoo-0123456789abcdef.so", "dylib"),
            ("release/deps/foo-0123456789abcdef.dll", "dylib"),
            ("release/deps/foo-0123456789abcdef.exe", "binary"),
            (
                "release/deps/foo-0123456789abcdef.dwo",
                "debuginfo (split DWARF)",
            ),
            (
                "release/deps/foo-0123456789abcdef.dwp",
                "debuginfo (split DWARF)",
            ),
            (
                "release/deps/foo-0123456789abcdef.dSYM/Contents/Resources/DWARF/foo",
                "debuginfo (dSYM)",
            ),
            ("doc/foo/index.html", "doc"),
            ("package/foo.crate", "package"),
            (
                "release/.fingerprint/foo-0123456789abcdef/lib-foo.json",
                "fingerprint",
            ),
            (
                "release/incremental/foo-2k1x9/s-a-b/dep-graph.bin",
                "incremental",
            ),
            (
                "release/build/foo-0123456789abcdef/out/generated.rs",
                "build script output",
            ),
            (
                "release/build/foo-0123456789abcdef/build_script_build-0123456789abcdef",
                "build script executable",
            ),
            (
                "release/build/foo-0123456789abcdef/build-script-build",
                "build script executable",
            ),
            ("release/deps/notes-0123456789abcdef", "other"),
            ("CACHEDIR.TAG", "other"),
        ];
        for (path, kind) in cases {
            assert_eq!(k(path), kind, "{path}");
        }
        assert_eq!(
            classify(
                Path::new("release/deps/runner-0123456789abcdef"),
                true,
                false
            ),
            "binary",
            "a file with no extension is a binary when it is executable"
        );
    }

    #[test]
    fn classifies_the_v2_units_by_what_the_unit_is() {
        let unit = "debug/build/dep/0123456789abcdef";
        let k = |p: &str, script| classify(Path::new(p), false, script);
        assert_eq!(
            k(&format!("{unit}/out/libdep-0123456789abcdef.rlib"), false),
            "rlib"
        );
        assert_eq!(
            k(&format!("{unit}/out/libdep-0123456789abcdef.rmeta"), false),
            "rmeta"
        );
        assert_eq!(
            k(&format!("{unit}/out/dep-0123456789abcdef.1x.rcgu.o"), false),
            "object file"
        );
        assert_eq!(
            k(&format!("{unit}/fingerprint/lib-dep.json"), false),
            "fingerprint"
        );
        assert_eq!(
            k(&format!("{unit}/out/generated.rs"), true),
            "build script output"
        );
        assert_eq!(
            k(&format!("{unit}/run/stdout"), false),
            "build script output"
        );
        assert_eq!(
            k(&format!("{unit}/out/build_script_build"), false),
            "build script executable"
        );
    }

    #[test]
    fn names_the_crate_in_both_layouts() {
        let c = |p: &str| crate_of(Path::new(p));
        assert_eq!(
            c("debug/build/dep/0123456789abcdef/out/libdep-0123456789abcdef.rlib").as_deref(),
            Some("dep")
        );
        assert_eq!(
            c("debug/build/dep/0123456789abcdef/run/stdout").as_deref(),
            Some("dep")
        );
        assert_eq!(
            c("debug/deps/libstatic-0123456789abcdef.a").as_deref(),
            Some("static")
        );
        assert_eq!(
            c("debug/.fingerprint/serde_json-0123456789abcdef/lib-serde_json.json").as_deref(),
            Some("serde_json")
        );
        assert_eq!(c("doc/foo/index.html"), None);
        assert_eq!(c("CACHEDIR.TAG"), None);
    }

    #[test]
    fn a_report_accounts_for_each_added_path_once() {
        let mut r = Report::default();
        let layout = Layout {
            profiles: HashSet::from([PathBuf::from("debug")]),
            packages: HashMap::from([("notes".to_string(), "notebook".to_string())]),
        };
        r.add(Path::new(""), "directories", 4096, &layout);
        r.add(
            Path::new("debug/deps/libfoo-0123456789abcdef.rlib"),
            "rlib",
            8192,
            &layout,
        );
        r.add(
            Path::new("debug/deps/notes-0123456789abcdef.txt"),
            "other",
            1024,
            &layout,
        );

        assert_eq!(r.total, 13_312);
        assert_eq!(r.by_kind["rlib"], 8192);
        assert_eq!(r.by_kind["other"], 1024);
        assert_eq!(r.by_profile["(not in a profile)"], 4096);
        assert_eq!(r.by_profile["debug"], 9216);
        assert_eq!(r.by_crate["foo"], 8192);
        // `notes` is a target of the package `notebook`.
        assert_eq!(r.by_crate["notebook"], 1024);
        assert_eq!(r.by_crate.len(), 2);
    }

    #[test]
    fn sorts_by_size_then_by_name() {
        let m = BTreeMap::from([
            ("b".to_string(), 2),
            ("a".to_string(), 2),
            ("c".to_string(), 3),
        ]);
        let names: Vec<&str> = sorted(&m).iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    /// A GNU `ar` header is 60 bytes: name, mtime, uid, gid, mode, size, magic.
    fn ar_header(name: &str, len: usize) -> [u8; 60] {
        let fields = [
            format!("{name}/"),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "100644".to_string(),
            len.to_string(),
        ];
        let widths = [16, 12, 6, 6, 8, 10];
        let mut header = [b' '; 60];
        let mut offset = 0;
        for (field, width) in fields.iter().zip(widths) {
            assert!(field.len() <= width);
            header[offset..offset + field.len()].copy_from_slice(field.as_bytes());
            offset += width;
        }
        header[58] = b'`';
        header[59] = b'\n';
        header
    }

    fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = b"!<arch>\n".to_vec();
        for &(name, data) in members {
            out.extend_from_slice(&ar_header(name, data.len()));
            out.extend_from_slice(data);
            if data.len() % 2 == 1 {
                out.push(b'\n');
            }
        }
        out
    }

    #[test]
    fn reads_metadata_out_of_an_rlib_archive() {
        let data = archive(&[("lib.rmeta", &[0xab; 100]), ("object", &[0; 13])]);
        let p = parts(io::Cursor::new(&data), true);
        assert_eq!(p.metadata, 100);
        assert_eq!(p.debuginfo, 0);

        // The same bytes are not an rlib when the caller says so: the archive
        // is then parsed as an object file, which fails harmlessly.
        let p = parts(io::Cursor::new(&data), false);
        assert_eq!(p.metadata, 0);
        assert_eq!(p.debuginfo, 0);
    }

    #[test]
    fn ignores_bytes_that_are_not_an_object_or_an_archive() {
        for data in [&b""[..], &b"not an object file"[..], &[0u8; 64][..]] {
            let p = parts(io::Cursor::new(data), false);
            assert_eq!(p.metadata, 0);
            assert_eq!(p.debuginfo, 0);
        }
        let mut p = Parts::default();
        object_parts(&[0u8; 17][..], &mut p);
        assert_eq!(p.metadata, 0);
        assert_eq!(p.debuginfo, 0);
    }
}
