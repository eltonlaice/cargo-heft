//! `cargo heft disk`: what Rust keeps on disk outside one target directory,
//! the Cargo home and the rustup home, read-only. See docs/disk.md.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
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
    /// The command that removes it, run by the user. None for `Keep`.
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
        let (bytes, id, _) = disk_usage(&md);
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
            let (bytes, id, _) = disk_usage(&md);
            if seen.insert(id) {
                total += bytes;
                *sizes.entry("other".into()).or_default() += bytes;
            }
        }
        du(&path, &path, seen, unreadable, &mut |rel, bytes| {
            total += bytes;
            let name = name_of(rel).unwrap_or_else(|| "other".into());
            *sizes.entry(name).or_default() += bytes;
        });
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

/// The toolchain `rustup default` names, read from `settings.toml`.
fn default_toolchain(rustup: &Path) -> Option<String> {
    let text = fs::read_to_string(rustup.join("settings.toml")).ok()?;
    text.lines().find_map(|l| {
        let v = l.trim().strip_prefix("default_toolchain")?.trim_start();
        Some(v.strip_prefix('=')?.trim().trim_matches('"').to_string())
    })
}

pub fn inventory(cargo_home: PathBuf, rustup_home: PathBuf) -> (Vec<Home>, Vec<PathBuf>) {
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
    let default = default_toolchain(&rustup_home);
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
            match default.as_deref() == Some(t) {
                true => (Class::Keep, None),
                false => (
                    Class::Download,
                    Some(format!("rustup toolchain uninstall {t}")),
                ),
            }
        },
    );
    (vec![cargo, rustup], unreadable)
}

pub fn run(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    const USAGE: &str = "Usage: cargo heft disk\n\n  \
        Measures the Cargo home (CARGO_HOME, default ~/.cargo) and the rustup\n  \
        home (RUSTUP_HOME, default ~/.rustup) by part, and says what removing\n  \
        each part would cost. It reads only; nothing is removed.";
    if let Some(a) = args.next() {
        return match a.to_str() {
            Some("-h" | "--help") => {
                say!("{USAGE}");
                Ok(())
            }
            _ => Err(format!(
                "unknown argument `{}` for `cargo heft disk`\n{USAGE}",
                a.to_string_lossy()
            )),
        };
    }
    let (homes, unreadable) = inventory(
        home_dir("CARGO_HOME", ".cargo")?,
        home_dir("RUSTUP_HOME", ".rustup")?,
    );
    let mut grand = 0;
    for h in &homes {
        if !h.found {
            say!("{}: {} (not found)\n", h.title, h.path.display());
            continue;
        }
        grand += h.total;
        say!("{}: {} ({})", h.title, h.path.display(), human(h.total));
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
        "\n  rebuilt   recreated from another cache on this machine, no network\n  \
         download  fetched again: the next build needs the network, offline builds fail\n  \
         keep      installed tools, settings, credentials, the default toolchain,\n            \
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
