//! `cargo heft disk`: what Rust keeps on disk outside one target directory,
//! the Cargo home and the rustup home, read-only. See docs/disk.md.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs};

use crate::{disk_usage, human};

/// What removing a part costs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Class {
    /// Recreated from another cache on this machine, no network.
    Rebuilt,
    /// Fetched again: the next build needs the network.
    Download,
    /// Installed tools, settings, credentials, the default toolchain, or a
    /// file this tool does not recognise. Never recommended for removal.
    Keep,
}

impl Class {
    fn label(self) -> &'static str {
        match self {
            Class::Rebuilt => "rebuilt",
            Class::Download => "download",
            Class::Keep => "keep",
        }
    }
}

pub struct Part {
    pub name: String,
    pub bytes: u64,
    pub class: Class,
    /// The command that removes it, run by the user; for `Keep`, why it is
    /// kept when that is worth saying, such as "(in use: default)".
    pub command: Option<String>,
}

pub struct Home {
    pub title: &'static str,
    pub path: PathBuf,
    pub found: bool,
    pub total: u64,
    pub parts: Vec<Part>,
}

const GC: &str = "cargo +nightly -Zgc clean gc";

/// The parts of the Cargo home this tool recognises.
const CARGO_PARTS: &[(&str, Class)] = &[
    ("registry/index", Class::Download),
    ("registry/cache", Class::Download),
    ("registry/src", Class::Rebuilt),
    ("git/db", Class::Download),
    ("git/checkouts", Class::Rebuilt),
    ("bin", Class::Keep),
];

/// Bytes on disk under `root`, attributed to the path of each entry relative
/// to `root`. Every inode once (`seen`), symlinks counted and not followed,
/// and a directory that cannot be read is listed in `unreadable` instead of
/// stopping the walk: a home directory is not ours to fail on.
fn du(
    root: &Path,
    dir: &Path,
    seen: &mut HashSet<(u64, u64)>,
    unreadable: &mut Vec<PathBuf>,
    put: &mut impl FnMut(&Path, u64),
) {
    let Ok(entries) = fs::read_dir(dir) else {
        unreadable.push(dir.to_path_buf());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(md) = fs::symlink_metadata(&path) else {
            unreadable.push(path);
            continue;
        };
        let (bytes, id, _) = disk_usage(&path, &md);
        if id == (0, 0) || seen.insert(id) {
            put(path.strip_prefix(root).unwrap_or(&path), bytes);
        }
        if md.is_dir() {
            du(root, &path, seen, unreadable, put);
        }
    }
}

/// Measures `path` and sorts its bytes into the parts `name_of` gives each
/// relative path (None: "other").
fn measure(
    title: &'static str,
    path: PathBuf,
    seen: &mut HashSet<(u64, u64)>,
    unreadable: &mut Vec<PathBuf>,
    name_of: impl Fn(&Path) -> Option<String>,
    classify: impl Fn(&str) -> (Class, Option<String>),
) -> Home {
    let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
    let found = path.is_dir();
    let mut total = 0;
    if found {
        if let Ok(md) = fs::symlink_metadata(&path) {
            let (bytes, id, _) = disk_usage(&path, &md);
            if id == (0, 0) || seen.insert(id) {
                total += bytes;
                *sizes.entry("other".into()).or_default() += bytes;
            }
        }
        // A large home takes a while; on a terminal, show that it is moving.
        let progress = io::stderr().is_terminal();
        let (mut entries, mut shown) = (0u64, Instant::now());
        du(&path, &path, seen, unreadable, &mut |rel, bytes| {
            total += bytes;
            let name = name_of(rel).unwrap_or_else(|| "other".into());
            *sizes.entry(name).or_default() += bytes;
            entries += 1;
            if progress && entries % 1024 == 0 && shown.elapsed() > Duration::from_millis(200) {
                eprint!("\rmeasuring {}: {}\x1b[K", path.display(), human(total));
                shown = Instant::now();
            }
        });
        if progress {
            eprint!("\r\x1b[K");
        }
    }
    let mut parts: Vec<Part> = sizes
        .into_iter()
        .map(|(name, bytes)| {
            let (class, command) = match name.as_str() {
                "other" => (Class::Keep, None),
                n => classify(n),
            };
            Part {
                name,
                bytes,
                class,
                command,
            }
        })
        .collect();
    parts.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    Home {
        title,
        path,
        found,
        total,
        parts,
    }
}

fn home_dir(var: &str, default: &str) -> Result<PathBuf, String> {
    match env::var_os(var) {
        Some(p) if !p.is_empty() => Ok(PathBuf::from(p)),
        _ => env::home_dir()
            .map(|h| h.join(default))
            .ok_or_else(|| format!("cannot find the home directory; set {var}")),
    }
}

/// `"value"` out of a `key = "value"` line.
fn quoted_value(line: &str, key: &str) -> Option<String> {
    let v = line.trim().strip_prefix(key)?.trim_start();
    Some(v.strip_prefix('=')?.trim().trim_matches('"').to_string())
}

/// Toolchains rustup would pick here, each with why: the default, an
/// override set with `rustup override`, `RUSTUP_TOOLCHAIN`, or a
/// `rust-toolchain(.toml)` in `cwd` or above it. The names are as written
/// (`stable`, `1.88.0`), not the directory names with the host triple.
pub fn toolchains_in_use(rustup: &Path, cwd: &Path) -> Vec<(String, String)> {
    let mut used = Vec::new();
    let settings = fs::read_to_string(rustup.join("settings.toml")).unwrap_or_default();
    let mut in_overrides = false;
    for line in settings.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_overrides = line == "[overrides]";
        } else if in_overrides {
            if let Some((path, tc)) = line.split_once('=') {
                let (path, tc) = (path.trim().trim_matches('"'), tc.trim().trim_matches('"'));
                used.push((tc.to_string(), format!("override for {path}")));
            }
        } else if let Some(tc) = quoted_value(line, "default_toolchain") {
            used.push((tc, "default".to_string()));
        }
    }
    if let Some(tc) = env::var_os("RUSTUP_TOOLCHAIN").filter(|t| !t.is_empty()) {
        used.push((
            tc.to_string_lossy().into_owned(),
            "RUSTUP_TOOLCHAIN".to_string(),
        ));
    }
    for dir in cwd.ancestors() {
        for name in ["rust-toolchain.toml", "rust-toolchain"] {
            let Ok(text) = fs::read_to_string(dir.join(name)) else {
                continue;
            };
            let tc = text
                .lines()
                .find_map(|l| quoted_value(l, "channel"))
                .or_else(|| {
                    // The old format: the file holds only the toolchain name.
                    let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
                    (!first.contains(['=', '['])).then(|| first.to_string())
                });
            if let Some(tc) = tc {
                used.push((tc, format!("pinned by {}", dir.join(name).display())));
            }
        }
    }
    used
}

/// Whether the toolchain directory `dir` (`stable-aarch64-apple-darwin`) is
/// the toolchain `name` (`stable`) names: the name, then `-` and the host
/// triple, which starts with a letter. A date starts with a digit, so
/// `nightly` does not name `nightly-2026-11-01-aarch64-apple-darwin`, and
/// `1.8` does not name `1.80.0-...`.
fn is_toolchain(dir: &str, name: &str) -> bool {
    dir == name
        || dir
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|triple| triple.starts_with(|c: char| c.is_ascii_alphabetic()))
}

/// The Cargo home, the rustup home, then the target directories under each
/// of `project_dirs`. One inode set for all: a file in two of them is
/// counted once, in the first.
pub fn inventory(
    cargo_home: PathBuf,
    rustup_home: PathBuf,
    project_dirs: Vec<PathBuf>,
) -> (Vec<Home>, Vec<PathBuf>) {
    let mut seen = HashSet::new();
    let mut unreadable = Vec::new();
    let cargo = measure(
        "Cargo home",
        cargo_home,
        &mut seen,
        &mut unreadable,
        |rel| {
            CARGO_PARTS
                .iter()
                .find(|(p, _)| rel.starts_with(p))
                .map(|(p, _)| p.to_string())
        },
        |name| {
            let class = CARGO_PARTS.iter().find(|(p, _)| *p == name).unwrap().1;
            (class, (class != Class::Keep).then(|| GC.to_string()))
        },
    );
    let cwd = env::current_dir().unwrap_or_default();
    let used = toolchains_in_use(&rustup_home, &cwd);
    let rustup = measure(
        "rustup home",
        rustup_home,
        &mut seen,
        &mut unreadable,
        |rel| {
            let mut c = rel.components();
            (c.next()?.as_os_str() == "toolchains")
                .then(|| c.next())
                .flatten()
                .map(|t| format!("toolchains/{}", t.as_os_str().to_string_lossy()))
        },
        |name| {
            let t = name.trim_start_matches("toolchains/");
            let why: Vec<&str> = used
                .iter()
                .filter(|(tc, _)| is_toolchain(t, tc))
                .map(|(_, why)| why.as_str())
                .collect();
            match why.is_empty() {
                true => (
                    Class::Download,
                    Some(format!("rustup toolchain uninstall {t}")),
                ),
                false => (Class::Keep, Some(format!("(in use: {})", why.join(", ")))),
            }
        },
    );
    let mut homes = vec![cargo, rustup];
    for d in project_dirs {
        homes.push(projects(d, &mut seen, &mut unreadable));
    }
    (homes, unreadable)
}

/// `path` as a shell word: quoted when it has a space or a quote in it.
fn shell_word(path: &Path) -> String {
    let s = path.to_string_lossy();
    match s.contains(|c: char| c.is_whitespace() || "'\"$`\\".contains(c)) {
        true => format!("'{}'", s.replace('\'', r"'\''")),
        false => s.into_owned(),
    }
}

/// The workspace root and the target and build directories `cargo metadata`
/// gives for `manifest`, which may be any member. They follow
/// `CARGO_TARGET_DIR`, `build.target-dir` and `build.build-dir` as Cargo
/// itself would. Cargo reads `.cargo/config.toml` from the directory it runs
/// in, not the manifest's, so it runs in the manifest's directory.
/// `--offline --no-deps`: nothing is fetched or resolved. A project that
/// pins a toolchain which is not installed would make rustup download it, so
/// auto-install is off: Cargo fails instead, and the project falls back to
/// its `target/`.
fn workspace_of(manifest: &Path) -> Option<(PathBuf, Vec<PathBuf>, Vec<PathBuf>)> {
    let out = crate::cargo()
        .current_dir(manifest.parent()?)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stderr(std::process::Stdio::null())
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let path = |k: &str| v[k].as_str().map(PathBuf::from);
    let root = path("workspace_root")?;
    let mut dirs = vec![path("target_directory")?];
    if let Some(b) = path("build_directory").filter(|b| *b != dirs[0]) {
        dirs.push(b);
    }
    let members = v["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Path::new(p["manifest_path"].as_str()?)
                .parent()
                .map(Path::to_path_buf)
        })
        .collect();
    Some((root, dirs, members))
}

/// Cargo workspaces under `dir`, each with its target and build directories.
/// A `Cargo.toml` that belongs to a workspace already found is skipped.
/// Never entered: symlinks, hidden directories, and directories Cargo made
/// (they hold a CACHEDIR.TAG; `target/package` has `Cargo.toml` files of
/// its own).
fn find_workspaces(
    dir: &Path,
    found: &mut Vec<(PathBuf, Vec<PathBuf>)>,
    members: &mut HashSet<PathBuf>,
    unreadable: &mut Vec<PathBuf>,
) {
    if dir.join("CACHEDIR.TAG").exists() {
        return;
    }
    let manifest = dir.join("Cargo.toml");
    if manifest.is_file() && !members.contains(dir) {
        match workspace_of(&manifest) {
            Some((root, dirs, of)) => {
                members.extend(of);
                members.insert(root.clone());
                members.insert(dir.to_path_buf());
                found.push((root, dirs));
            }
            // A manifest Cargo cannot read: its `target/`, if there is one.
            None => {
                members.insert(dir.to_path_buf());
                found.push((dir.to_path_buf(), vec![dir.join("target")]));
            }
        }
    }
    let Ok(entries) = fs::read_dir(dir) else {
        unreadable.push(dir.to_path_buf());
        return;
    };
    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    subdirs.sort();
    for d in subdirs {
        find_workspaces(&d, found, members, unreadable);
    }
}

/// The target directories of the workspaces under `dir`, one part each.
/// Several workspaces that share one target directory (`CARGO_TARGET_DIR`)
/// give one part, counted once.
fn projects(dir: PathBuf, seen: &mut HashSet<(u64, u64)>, unreadable: &mut Vec<PathBuf>) -> Home {
    let mut found = Vec::new();
    if dir.is_dir() {
        find_workspaces(&dir, &mut found, &mut HashSet::new(), unreadable);
    }
    // Cargo gives real paths (/private/var/... on macOS for /var/...).
    let base = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    let mut measured = HashSet::new();
    let (mut parts, mut total) = (Vec::new(), 0);
    for (root, dirs) in found {
        for d in dirs {
            // Not built yet, or already counted for another workspace.
            let Ok(real) = d.canonicalize() else { continue };
            if !real.is_dir() || !measured.insert(real.clone()) {
                continue;
            }
            let mut bytes = 0;
            if let Ok(md) = fs::symlink_metadata(&real) {
                let (b, id, _) = disk_usage(&real, &md);
                if id == (0, 0) || seen.insert(id) {
                    bytes += b;
                }
            }
            du(&real, &real, seen, unreadable, &mut |_, b| bytes += b);
            total += bytes;
            let name = real.strip_prefix(&base).unwrap_or(&real);
            parts.push(Part {
                name: name.to_string_lossy().into_owned(),
                bytes,
                class: Class::Rebuilt,
                command: Some(format!(
                    "cargo clean --manifest-path {}",
                    shell_word(&root.join("Cargo.toml"))
                )),
            });
        }
    }
    parts.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    Home {
        title: "Target directories",
        found: dir.is_dir(),
        path: dir,
        total,
        parts,
    }
}

/// Version of `cargo heft disk --json`, separate from the target report's:
/// raised when a key is removed or renamed or a value changes meaning.
const DISK_SCHEMA_VERSION: u32 = 1;

fn json_report(homes: &[Home], unreadable: &[PathBuf]) -> serde_json::Value {
    let homes: Vec<_> = homes
        .iter()
        .map(|h| {
            let parts: Vec<_> = h
                .parts
                .iter()
                .map(|p| {
                    // For `keep`, `command` carries why it is kept, in brackets.
                    let why = (p.class == Class::Keep)
                        .then_some(p.command.as_deref())
                        .flatten()
                        .map(|c| c.trim_start_matches("(in use: ").trim_end_matches(')'));
                    serde_json::json!({
                        "name": p.name,
                        "bytes": p.bytes,
                        "class": p.class.label(),
                        "command": if p.class == Class::Keep { None } else { p.command.clone() },
                        "kept_because": why,
                    })
                })
                .collect();
            serde_json::json!({
                "name": match h.title {
                    "Cargo home" => "cargo",
                    "rustup home" => "rustup",
                    _ => "projects",
                },
                "path": h.path.to_string_lossy(),
                "found": h.found,
                "total_bytes": h.total,
                "parts": parts,
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": DISK_SCHEMA_VERSION,
        "total_bytes": homes.iter().map(|h| h["total_bytes"].as_u64().unwrap_or(0)).sum::<u64>(),
        "homes": homes,
        "unreadable": unreadable.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
    })
}

pub fn run(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    const USAGE: &str = "Usage: cargo heft disk [--projects <DIR>]... [--json]\n\n  \
        Measures the Cargo home (CARGO_HOME, default ~/.cargo) and the rustup\n  \
        home (RUSTUP_HOME, default ~/.rustup) by part, and says what removing\n  \
        each part would cost. It reads only; nothing is removed.\n\n  \
        --projects <DIR>  also the target directories of the Cargo workspaces\n                    \
        under DIR (repeatable; the home directory is never searched\n                    \
        unless named)\n  \
        --json            machine-readable output (schema in docs/disk.md)";
    let mut json = false;
    let mut project_dirs: Vec<PathBuf> = Vec::new();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        if let Some(d) = a.to_str().and_then(|s| s.strip_prefix("--projects=")) {
            project_dirs.push(PathBuf::from(d));
            continue;
        }
        match a.to_str() {
            Some("--projects") => match args.next() {
                Some(d) => project_dirs.push(PathBuf::from(d)),
                None => return Err(format!("--projects needs a directory\n{USAGE}")),
            },
            Some("-h" | "--help") => {
                say!("{USAGE}");
                return Ok(());
            }
            Some("--json") => json = true,
            _ => {
                return Err(format!(
                    "unknown argument `{}` for `cargo heft disk`\n{USAGE}",
                    a.to_string_lossy()
                ));
            }
        }
    }
    let (homes, unreadable) = inventory(
        home_dir("CARGO_HOME", ".cargo")?,
        home_dir("RUSTUP_HOME", ".rustup")?,
        project_dirs,
    );
    if json {
        say!("{:#}", json_report(&homes, &unreadable));
        return Ok(());
    }
    let mut grand = 0;
    for h in &homes {
        if !h.found {
            say!("{}: {} (not found)\n", h.title, h.path.display());
            continue;
        }
        grand += h.total;
        say!(
            "{}: {} ({}, {:.1} MiB in the units du -h uses)",
            h.title,
            h.path.display(),
            human(h.total),
            h.total as f64 / 1_048_576.0
        );
        if h.parts.is_empty() && h.title == "Target directories" {
            say!("  no built Cargo workspace found\n");
            continue;
        }
        let w = h.parts.iter().map(|p| p.name.len()).max().unwrap_or(0);
        for p in &h.parts {
            let cmd = p.command.as_deref().unwrap_or("");
            let line = format!(
                "  {:<w$}  {:>10}  {:<8}  {cmd}",
                p.name,
                human(p.bytes),
                p.class.label()
            );
            say!("{}", line.trim_end());
        }
        say!("");
    }
    say!("Total: {}", human(grand));
    say!(
        "\n  rebuilt   recreated on this machine, no network: from another cache, or\n            \
         for a target directory, by the next build\n  \
         download  fetched again: the next build needs the network, offline builds fail\n  \
         keep      installed tools, settings, credentials, a toolchain in use here\n            \
         (default, override, RUSTUP_TOOLCHAIN or rust-toolchain.toml),\n            \
         or files cargo-heft does not recognise\n\n\
         Nothing was removed. `cargo clean gc` is nightly-only for now; Cargo also\n\
         cleans these caches by itself."
    );
    if !unreadable.is_empty() {
        eprintln!(
            "cargo-heft: {} path(s) could not be read and are not counted, such as {}",
            unreadable.len(),
            unreadable[0].display()
        );
    }
    Ok(())
}
