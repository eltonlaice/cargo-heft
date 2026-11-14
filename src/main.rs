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

  --target-dir <DIR>  directory to measure (default: the workspace's target dir)
  --top <N>           crates to list (default 15)
  --json              machine-readable output";

/// What a path inside a target directory is, judged by its name so that it
/// holds for both the classic layout and Cargo's new build-dir layout.
fn classify(rel: &Path, executable: bool) -> &'static str {
    let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let name = comps.last().copied().unwrap_or("");
    let has = |c: &str| comps.contains(&c);
    if has("incremental") {
        return "incremental";
    }
    if has(".fingerprint") {
        return "fingerprint";
    }
    if comps.iter().any(|c| c.ends_with(".dSYM")) {
        return "debuginfo (dSYM)";
    }
    if has("build") && has("out") {
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
    (hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then_some(name)
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
}

struct File {
    rel: PathBuf,
    bytes: u64,
    id: (u64, u64),
    executable: bool,
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<File>, r: &mut Report) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let md = fs::symlink_metadata(&path)?;
        let (bytes, id, executable) = disk_usage(&md);
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if md.is_dir() {
            r.add(&rel, "directories", bytes);
            walk(root, &path, files, r)?;
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

fn measure(root: &Path) -> io::Result<Report> {
    let mut r = Report::default();
    let mut files = Vec::new();
    let (bytes, ..) = disk_usage(&fs::symlink_metadata(root)?);
    r.add(Path::new(""), "directories", bytes);
    walk(root, root, &mut files, &mut r)?;
    // rustc hard-links objects and metadata between `deps` and `incremental`.
    // Count each inode once, as the artifact outside `incremental`, so that
    // "incremental" is what deleting that directory would actually free.
    let in_incremental = |f: &File| f.rel.iter().any(|c| c == "incremental");
    files.sort_by(|a, b| (in_incremental(a), &a.rel).cmp(&(in_incremental(b), &b.rel)));
    let mut seen = HashSet::new();
    for f in &files {
        if cfg!(unix) && !seen.insert(f.id) {
            continue;
        }
        r.add(&f.rel, classify(&f.rel, f.executable), f.bytes);
    }
    Ok(r)
}

fn default_target_dir() -> Result<PathBuf, String> {
    if let Some(d) = env::var_os("CARGO_TARGET_DIR") {
        return Ok(d.into());
    }
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
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
        println!("  {name:<28} {:>9.1} MB {pct:>6.1}%", *bytes as f64 / 1e6);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("heft") {
        args.next(); // invoked as `cargo heft`
    }
    let (mut target, mut top, mut json) = (None, 15usize, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--target-dir" => target = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--top" => top = args.next().and_then(|n| n.parse().ok()).ok_or(USAGE)?,
            "--json" => json = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown argument `{a}`\n{USAGE}")),
        }
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
            "by_profile": r.by_profile,
            "by_crate": r.by_crate,
        });
        println!("{out:#}");
    } else {
        println!(
            "{}: {:.1} MB on disk",
            target.display(),
            r.total as f64 / 1e6
        );
        print_table("by kind", &r.by_kind, r.total, usize::MAX);
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
        let k = |p: &str| classify(Path::new(p), false);
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
        assert_eq!(classify(Path::new("debug/rg"), true), "binary");
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
