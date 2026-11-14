//! cargo-heft: where the bytes of a Cargo target directory go.
//!
//! Sizes are on-disk (allocated blocks), every inode is counted once, and
//! directories are included, so the total matches `du -s`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs, io};

const USAGE: &str = "\
Usage: cargo heft [--target-dir <DIR>] [--top <N>] [--json]
       cargo heft --what-if [--profile <NAME>] [--json] [-- <cargo build args>]

  --target-dir <DIR>  directory to measure (default: the workspace's target dir)
  --top <N>           crates to list (default 15)
  --json              machine-readable output
  --what-if           build this workspace from scratch once per profile setting,
                      in a temporary directory, and compare the sizes
  --profile <NAME>    profile for --what-if (default: dev)
  -V, --version       print the version";

/// Profile settings `--what-if` tries, as (key, value) pairs of `[profile.*]`.
const SCENARIOS: &[(&str, &[(&str, &str)])] = &[
    ("as configured", &[]),
    (
        "debug = \"line-tables-only\"",
        &[("debug", "line-tables-only")],
    ),
    ("debug = false", &[("debug", "false")]),
    ("incremental = false", &[("incremental", "false")]),
    (
        "line-tables-only, no incremental",
        &[("debug", "line-tables-only"), ("incremental", "false")],
    ),
];

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
    let classic_out = after(1).is_some_and(|c| strip_hash(c).is_some()) && after(2) == Some("out");
    if classic_out || (v2 && (after(3) == Some("run") || script_unit)) {
        return "build script output";
    }
    if name.starts_with("build_script_") || name.starts_with("build-script-") {
        return "build script executable";
    }
    match comps.first().copied() {
        Some("doc") => return "doc",
        Some("package") => return "package",
        _ => {}
    }
    match rel.extension().and_then(|e| e.to_str()) {
        Some("rlib") => "rlib",
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
        // incremental/<crate>-<base36 hash>/...
        return comps
            .get(i + 1)
            .and_then(|c| c.rsplit_once('-'))
            .map(|(n, _)| norm(n));
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

fn object_parts(data: &[u8], p: &mut Parts) {
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

/// Looks inside an `.rlib` (an archive of metadata and object files) or a
/// single object file, executable or dylib.
fn parts(data: &[u8], is_rlib: bool) -> Parts {
    let mut p = Parts::default();
    match object::read::archive::ArchiveFile::parse(data) {
        Ok(archive) if is_rlib => {
            for member in archive.members().flatten() {
                let Ok(bytes) = member.data(data) else {
                    continue;
                };
                if member.name() == b"lib.rmeta" {
                    p.metadata += bytes.len() as u64;
                } else {
                    object_parts(bytes, &mut p);
                }
            }
        }
        _ => object_parts(data, &mut p),
    }
    p
}

struct File {
    rel: PathBuf,
    bytes: u64,
    id: (u64, u64),
    executable: bool,
}

fn walk(
    root: &Path,
    dir: &Path,
    files: &mut Vec<File>,
    script_units: &mut HashSet<PathBuf>,
    r: &mut Report,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let md = fs::symlink_metadata(&path)?;
        let (bytes, id, executable) = disk_usage(&md);
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if md.is_dir() {
            r.add(&rel, "directories", bytes);
            if let (Some("run"), Some(unit)) =
                (rel.file_name().and_then(|n| n.to_str()), rel.parent())
            {
                script_units.insert(unit.to_path_buf());
            }
            walk(root, &path, files, script_units, r)?;
        } else {
            files.push(File {
                rel,
                bytes,
                id,
                executable,
            });
        }
    }
    Ok(())
}

impl Report {
    fn add(&mut self, rel: &Path, kind: &str, bytes: u64) {
        self.total += bytes;
        *self.by_kind.entry(kind.into()).or_default() += bytes;
        let mut comps = rel.iter().filter_map(|c| c.to_str());
        let profile = match (comps.next(), comps.next()) {
            (Some(first), Some(_)) => first,
            _ => "(top level)",
        };
        *self.by_profile.entry(profile.into()).or_default() += bytes;
        if let Some(c) = crate_of(rel) {
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
            "rlib" | "object file" | "binary" | "dylib" | "build script executable" => {
                // ponytail: reads each file whole; mmap if huge targets get slow.
                let data = fs::read(root.join(&f.rel)).unwrap_or_default();
                let p = parts(&data, kind == "rlib");
                // Scale logical bytes to the blocks the file occupies.
                let scale =
                    |n: u64| (n as u128 * f.bytes as u128 / data.len().max(1) as u128) as u64;
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
    let (bytes, ..) = disk_usage(&fs::symlink_metadata(root)?);
    r.add(Path::new(""), "directories", bytes);
    let mut script_units = HashSet::new();
    walk(root, root, &mut files, &mut script_units, &mut r)?;
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
        let kind = classify(&f.rel, f.executable, script_unit);
        r.add(&f.rel, kind, f.bytes);
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

fn what_if(profile: &str, extra: &[String], json: bool) -> Result<(), String> {
    let scratch = Scratch(env::temp_dir().join(format!("cargo-heft-{}", std::process::id())));
    let _ = cargo().arg("fetch").output(); // keep downloads out of the first timing
    let prefix = format!(
        "CARGO_PROFILE_{}_",
        profile.to_uppercase().replace('-', "_")
    );
    let mut rows = Vec::new();
    for (i, (name, settings)) in SCENARIOS.iter().enumerate() {
        eprintln!("building {}/{}: {name}", i + 1, SCENARIOS.len());
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
        rows.push((*name, *settings, total, seconds));
    }
    if json {
        let scenarios: Vec<_> = rows
            .iter()
            .map(|(name, settings, total, seconds)| {
                let settings: BTreeMap<_, _> = settings.iter().copied().collect();
                serde_json::json!({ "name": name, "settings": settings, "total_bytes": total, "build_seconds": seconds })
            })
            .collect();
        println!(
            "{:#}",
            serde_json::json!({ "profile": profile, "scenarios": scenarios })
        );
        return Ok(());
    }
    let base = rows[0].2;
    println!("fresh `{profile}` builds, settings as they would go in [profile.{profile}]\n");
    println!(
        "  {:<34} {:>10} {:>9} {:>8}",
        "setting", "size", "vs first", "build"
    );
    for (name, _, total, seconds) in &rows {
        let delta = if *total == base || base == 0 {
            String::new()
        } else {
            format!(
                "{:+.1}%",
                100.0 * (*total as f64 - base as f64) / base as f64
            )
        };
        println!(
            "  {name:<34} {:>10} {delta:>9} {seconds:>7.1}s",
            human(*total)
        );
    }
    println!(
        "\n  note: these are builds from scratch. `incremental = false` makes rebuilds after an"
    );
    println!("  edit slower, so it suits CI more than local work. Less debuginfo means a debugger");
    println!("  shows less: `line-tables-only` keeps backtraces with file and line.");
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

fn human(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..1_000_000 => format!("{:.1} kB", b / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", b / 1e6),
        _ => format!("{:.2} GB", b / 1e9),
    }
}

fn sorted(m: &BTreeMap<String, u64>) -> Vec<(&String, &u64)> {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v
}

fn print_table(title: &str, m: &BTreeMap<String, u64>, total: u64, top: usize) {
    println!("\n{title}");
    for (name, bytes) in sorted(m).into_iter().take(top) {
        let pct = if total == 0 {
            0.0
        } else {
            100.0 * *bytes as f64 / total as f64
        };
        println!("  {name:<28} {:>12} {pct:>6.1}%", human(*bytes));
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("heft") {
        args.next(); // invoked as `cargo heft`
    }
    let (mut target, mut top, mut json) = (None, 15usize, false);
    let (mut whatif, mut profile, mut extra) = (false, "dev".to_string(), Vec::new());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--target-dir" => target = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--top" => top = args.next().and_then(|n| n.parse().ok()).ok_or(USAGE)?,
            "--json" => json = true,
            "--what-if" => whatif = true,
            "--profile" => profile = args.next().ok_or(USAGE)?,
            "--" => extra.extend(args.by_ref()),
            "-V" | "--version" => {
                println!("cargo-heft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown argument `{a}`\n{USAGE}")),
        }
    }
    if whatif {
        return what_if(&profile, &extra, json);
    }
    let target = match target {
        Some(t) => t,
        None => default_target_dir()?,
    };
    let r = measure(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    if json {
        let out = serde_json::json!({
            "target_dir": target,
            "total_bytes": r.total,
            "by_kind": r.by_kind,
            "by_content": r.by_content,
            "duplicated_metadata_bytes": r.duplicated_metadata,
            "by_profile": r.by_profile,
            "by_crate": r.by_crate,
        });
        println!("{out:#}");
    } else {
        println!("{}: {} on disk", target.display(), human(r.total));
        print_table("by kind", &r.by_kind, r.total, usize::MAX);
        print_table("by content", &r.by_content, r.total, usize::MAX);
        if r.duplicated_metadata > 0 {
            println!(
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
    }
}
