//! Helpers shared by the integration tests.
//!
//! The tests drive the compiled `cargo-heft` binary as a black box: they run
//! it against target directories they build themselves, or against the small
//! workspace in `tests/fixture`, and check the reported numbers.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Map, Value};

/// The binary under test, built by Cargo for these tests.
pub const BIN: &str = env!("CARGO_BIN_EXE_cargo-heft");

/// Root of the crate under test.
pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The workspace built by the end-to-end tests.
pub fn fixture() -> PathBuf {
    repo().join("tests/fixture")
}

/// What a run of the binary produced.
pub struct Outcome {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    pub fn assert_ok(&self) -> &Self {
        assert!(
            self.status.success(),
            "cargo-heft failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            self.code(),
            self.stdout,
            self.stderr
        );
        assert!(
            !self.stderr.contains("panicked"),
            "cargo-heft panicked:\n{}",
            self.stderr
        );
        self
    }

    pub fn assert_failed(&self) -> &Self {
        assert_eq!(
            self.code(),
            Some(1),
            "expected exit code 1\nstdout:\n{}\nstderr:\n{}",
            self.stdout,
            self.stderr
        );
        assert!(
            !self.stderr.contains("panicked"),
            "cargo-heft panicked instead of reporting an error:\n{}",
            self.stderr
        );
        self
    }

    /// Parses stdout as JSON, insisting that the run succeeded.
    pub fn json(&self) -> Value {
        self.assert_ok();
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", self.stdout))
    }
}

/// Runs the binary in `dir`, without inheriting `CARGO_TARGET_DIR`.
pub fn run_in(dir: &Path, args: &[&str]) -> Outcome {
    run_with(dir, args, &[], &["CARGO_TARGET_DIR"])
}

/// Runs the binary in the repository root.
pub fn run(args: &[&str]) -> Outcome {
    run_in(&repo(), args)
}

/// Runs the binary with extra environment variables and variables to remove.
pub fn run_with(dir: &Path, args: &[&str], envs: &[(&str, &str)], remove: &[&str]) -> Outcome {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(dir).args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    for key in remove {
        cmd.env_remove(key);
    }
    let out = cmd.output().expect("run cargo-heft");
    Outcome {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Convenience wrapper: run `--json` in `dir` and parse the report.
pub fn report(dir: &Path, args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    run_in(dir, &all).json()
}

/// A scratch directory that is removed when the test ends.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a directory under Cargo's per-target temporary directory.
    pub fn new(tag: &str) -> Self {
        Self::in_base(Path::new(env!("CARGO_TARGET_TMPDIR")), tag)
    }

    /// Creates a directory outside the repository.
    ///
    /// Used when a test needs a current directory that is not inside a Cargo
    /// workspace, such as the default-target-dir tests.
    pub fn outside(tag: &str) -> Self {
        Self::in_base(&std::env::temp_dir(), tag)
    }

    fn in_base(base: &Path, tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("cargo-heft-test-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temporary directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Writes `bytes` to `root/rel`, creating the parent directories.
pub fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    path
}

/// Writes `len` bytes of `byte` to `root/rel`.
pub fn write_sized(root: &Path, rel: &str, len: usize, byte: u8) -> PathBuf {
    write_file(root, rel, &vec![byte; len])
}

/// Creates a directory and returns its path.
pub fn mkdir(root: &Path, rel: &str) -> PathBuf {
    let path = root.join(rel);
    fs::create_dir_all(&path).expect("create directory");
    path
}

/// Makes a file executable, where the platform has such a bit.
pub fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("stat file").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod file");
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Creates a hard link, creating the parent directory first.
pub fn hard_link(original: &Path, link: &Path) {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    fs::hard_link(original, link).expect("create hard link");
}

/// Creates a symbolic link pointing at `target`.
#[cfg(unix)]
pub fn symlink(target: &Path, link: &Path) {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    std::os::unix::fs::symlink(target, link).expect("create symlink");
}

/// On-disk bytes of one path, as the tool counts them.
pub fn disk_bytes(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::symlink_metadata(path).expect("stat").blocks() * 512
    }
    #[cfg(not(unix))]
    {
        fs::symlink_metadata(path).expect("stat").len()
    }
}

/// `du -sk`, the independent check used for totals.
#[cfg(unix)]
pub fn du_kib(path: &Path) -> u64 {
    let out = Command::new("du")
        .args(["-sk"])
        .arg(path)
        .output()
        .expect("run du");
    assert!(
        out.status.success(),
        "du failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .expect("du output")
        .parse()
        .expect("du output is a number")
}

/// Asserts that a total in bytes agrees with `du -sk` on the same directory.
#[cfg(unix)]
pub fn assert_matches_du(total: u64, path: &Path) {
    assert_eq!(
        total.div_ceil(1024),
        du_kib(path),
        "cargo-heft reports {total} bytes, du reports {} KiB",
        du_kib(path)
    );
}

fn number(map: &Map<String, Value>, key: &str) -> u64 {
    map.get(key)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{key} is not an unsigned integer in {map:?}"))
}

fn sum(map: &Map<String, Value>) -> u64 {
    map.values()
        .map(|v| v.as_u64().expect("view values are unsigned integers"))
        .sum()
}

/// Checks the JSON contract every report must satisfy.
pub fn assert_consistent(v: &Value) {
    let report = v.as_object().expect("report is a JSON object");
    let expected = [
        "by_content",
        "by_crate",
        "by_kind",
        "by_profile",
        "duplicated_metadata_bytes",
        "target_dir",
        "total_bytes",
    ];
    let mut keys: Vec<&str> = report.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, expected, "unexpected report shape: {v}");

    let total = report["total_bytes"]
        .as_u64()
        .expect("total_bytes is an unsigned integer");
    assert!(
        report["target_dir"].as_str().is_some_and(|s| !s.is_empty()),
        "target_dir is not a non-empty string: {}",
        report["target_dir"]
    );

    for view in ["by_kind", "by_content", "by_profile", "by_crate"] {
        let map = report[view]
            .as_object()
            .unwrap_or_else(|| panic!("{view} is not an object"));
        let total_of_view = sum(map);
        if view == "by_crate" {
            assert!(
                total_of_view <= total,
                "by_crate sums to {total_of_view}, more than the total {total}"
            );
        } else {
            assert_eq!(
                total_of_view, total,
                "{view} must account for every byte (total {total}): {map:?}"
            );
        }
    }

    let duplicate = report["duplicated_metadata_bytes"]
        .as_u64()
        .expect("duplicated_metadata_bytes is an unsigned integer");
    assert!(duplicate <= total, "duplicated metadata exceeds total");
}

/// Returns the data rows of a text table such as "by kind".
pub fn section_rows(stdout: &str, title: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut inside = false;
    for line in stdout.lines() {
        if inside {
            if line.trim().is_empty() {
                break;
            }
            rows.push(line.trim().to_string());
        } else if line == title {
            inside = true;
        }
    }
    rows
}

/// Returns the crate names listed under "largest crates".
pub fn crate_names(stdout: &str) -> Vec<String> {
    section_rows(stdout, "largest crates")
        .into_iter()
        .map(|row| {
            row.split_whitespace()
                .next()
                .expect("a table row has a name")
                .to_string()
        })
        .collect()
}
