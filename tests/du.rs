//! End-to-end tests: build the fixture workspace, then measure it.
//!
//! The total must match `du`, a real build must be recognised, and `--what-if`
//! must build and compare each profile setting.

mod common;

use std::path::Path;
use std::process::{Command, Stdio};

use common::*;

fn stdout(cmd: &mut Command) -> String {
    let out = cmd.output().expect("command runs");
    assert!(
        out.status.success(),
        "{:?}: {}",
        cmd,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn build_fixture(target: &Path, extra: &[&str]) {
    stdout(
        Command::new(env!("CARGO"))
            .arg("build")
            .args(extra)
            .current_dir(fixture())
            .env("CARGO_TARGET_DIR", target)
            // Cargo 1.100 turns incremental off when `CI` is set.
            .env("CARGO_INCREMENTAL", "1"),
    );
}

#[test]
fn total_matches_du_on_a_real_build() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixture-target");
    let _ = std::fs::remove_dir_all(&target);
    build_fixture(&target, &[]);

    let json = stdout(
        Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .args(["--json", "--target-dir"])
            .arg(&target),
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let total = v["total_bytes"].as_u64().unwrap();

    // du -sk reports 1024-byte units, rounded up.
    let du: u64 = stdout(Command::new("du").arg("-sk").arg(&target))
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        total.div_ceil(1024),
        du,
        "total {total} bytes vs du {du} KiB"
    );

    let kinds = v["by_kind"].as_object().unwrap();
    assert_eq!(
        kinds.values().map(|b| b.as_u64().unwrap()).sum::<u64>(),
        total
    );
    for kind in [
        "rlib",
        "rmeta",
        "incremental",
        "binary",
        "fingerprint",
        "build script executable",
        "build script output",
    ] {
        assert!(
            kinds.get(kind).is_some_and(|b| b.as_u64().unwrap() > 0),
            "no {kind} in {kinds:?}"
        );
    }
    assert!(v["by_crate"]["dep"].as_u64().unwrap() > 0);
    let crates = v["by_crate"].as_object().unwrap();
    assert!(
        !crates.keys().any(|c| c.starts_with("build_script")),
        "{crates:?}"
    );
    // The binary is `fx`, its package is `app`: one row, under the package.
    assert!(
        crates.contains_key("app") && !crates.contains_key("fx"),
        "{crates:?}"
    );
    let profiles: Vec<&String> = v["by_profile"].as_object().unwrap().keys().collect();
    assert_eq!(profiles, ["(not in a profile)", "debug"]);

    // The content view covers the same bytes, and sees inside the artifacts.
    let content = v["by_content"].as_object().unwrap();
    assert_eq!(
        content.values().map(|b| b.as_u64().unwrap()).sum::<u64>(),
        total
    );
    for part in [
        "debuginfo",
        "metadata (inside rlib/dylib)",
        "metadata (.rmeta files)",
    ] {
        assert!(
            content.get(part).is_some_and(|b| b.as_u64().unwrap() > 0),
            "no {part} in {content:?}"
        );
    }
    let duplicated = v["duplicated_metadata_bytes"].as_u64().unwrap();
    assert!(duplicated > 0);
    assert!(
        duplicated <= content["metadata (.rmeta files)"].as_u64().unwrap(),
        "duplicated metadata cannot exceed the standalone .rmeta files"
    );
    assert!(
        duplicated <= content["metadata (inside rlib/dylib)"].as_u64().unwrap(),
        "duplicated metadata cannot exceed the metadata inside the rlibs"
    );

    // The human-readable view says the same thing, and --top only truncates
    // the crate table.
    let text = stdout(
        Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .arg("--target-dir")
            .arg(&target),
    );
    for table in ["by kind", "by content", "by profile", "largest crates"] {
        assert!(text.contains(table), "no {table} in:\n{text}");
    }
    assert!(text.contains(" on disk"), "{}", text);
    assert!(
        text.contains("note:") && text.contains("stored twice"),
        "missing duplicated-metadata note:\n{text}"
    );
    let top = stdout(
        Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .arg("--target-dir")
            .arg(&target)
            .args(["--top", "1"]),
    );
    assert_eq!(
        crate_names(&top).len(),
        1,
        "--top 1 must list one crate:\n{top}"
    );
}

#[test]
fn release_profile_is_reported() {
    let scratch = TempDir::new("fixture-release");
    let target = scratch.join("target");
    build_fixture(&target, &["--release"]);

    let report = report(&fixture(), &["--target-dir", target.to_str().unwrap()]);
    assert_consistent(&report);
    assert!(
        report["by_profile"]["release"].as_u64().unwrap() > 0,
        "release profile missing from {}",
        report["by_profile"]
    );
    assert!(report["by_kind"]["binary"].as_u64().unwrap() > 0);
    #[cfg(unix)]
    assert_matches_du(report["total_bytes"].as_u64().unwrap(), &target);
}

#[test]
fn what_if_measures_each_setting() {
    let json = stdout(
        Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .args(["--what-if", "--json"])
            .current_dir(fixture())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CI"),
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["profile"], "dev");
    assert_eq!(v["schema_version"], 1);

    let scenarios = v["scenarios"].as_array().unwrap();
    assert_eq!(scenarios.len(), 5);
    let names: Vec<&str> = scenarios
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "as configured",
            "debug = \"line-tables-only\"",
            "debug = false",
            "incremental = false",
            "line-tables-only, no incremental",
        ]
    );
    let ids: Vec<&str> = scenarios
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "configured",
            "line-tables",
            "no-debug",
            "no-incremental",
            "line-tables-no-incremental"
        ]
    );
    assert_eq!(scenarios[0]["settings"], serde_json::json!({}));
    assert_eq!(
        scenarios[1]["settings"],
        serde_json::json!({ "debug": "line-tables-only" })
    );
    for scenario in scenarios {
        assert!(scenario["total_bytes"].as_u64().unwrap() > 0);
        assert!(scenario["build_seconds"].as_f64().unwrap() >= 0.0);
    }

    let size = |name: &str| {
        scenarios
            .iter()
            .find(|s| s["name"] == name)
            .unwrap_or_else(|| panic!("no scenario {name}"))["total_bytes"]
            .as_u64()
            .unwrap()
    };
    assert!(size("debug = false") < size("as configured"));
    // `<=`: where Cargo already builds without incremental, nothing changes.
    assert!(size("incremental = false") <= size("as configured"));
    assert!(size("line-tables-only, no incremental") <= size("debug = \"line-tables-only\""));
    assert!(size("debug = \"line-tables-only\"") < size("as configured"));
}

#[test]
fn a_failing_what_if_build_is_reported_and_the_scratch_dir_is_removed() {
    let child = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
        .args(["--what-if", "--"])
        .arg("--definitely-not-a-cargo-flag")
        .current_dir(fixture())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cargo-heft");
    let pid = child.id();
    let out = child.wait_with_output().expect("wait for cargo-heft");

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("building 1/5: as configured"),
        "expected progress on stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("cargo build failed (as configured)"),
        "expected a clear failure:\n{stderr}"
    );
    assert!(
        stderr.contains("--definitely-not-a-cargo-flag"),
        "expected cargo's explanation to mention the flag:\n{stderr}"
    );

    let scratch = std::env::temp_dir().join(format!("cargo-heft-{pid}"));
    assert!(
        !scratch.exists(),
        "the temporary build directory was left behind: {}",
        scratch.display()
    );
}

#[test]
fn only_builds_the_chosen_settings_in_the_order_given() {
    let json = stdout(
        Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .args([
                "--what-if",
                "--json",
                "--only",
                "no-debug,configured,no-debug",
            ])
            .current_dir(fixture())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CI"),
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let ids: Vec<&str> = v["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["no-debug", "configured"], "a repeated id builds once");
}

#[test]
fn ctrl_c_during_what_if_removes_the_scratch_dir() {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader};

        let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .args(["--what-if"])
            .current_dir(fixture())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CI")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cargo-heft");
        let pid = child.id();
        let mut stderr = BufReader::new(child.stderr.take().unwrap());
        let mut line = String::new();
        stderr.read_line(&mut line).unwrap();
        assert!(line.starts_with("building 1/5"), "{line}");
        // Only this process gets the signal here, not cargo as in a terminal,
        // so the first build runs to its end before the tool stops.
        let killed = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut stderr, &mut rest).unwrap();
        let status = child.wait().unwrap();

        assert_eq!(status.code(), Some(130), "{rest}");
        assert!(rest.contains("interrupted"), "{rest}");
        assert!(
            !rest.contains("building 2/5"),
            "it went on building: {rest}"
        );
        let scratch = std::env::temp_dir().join(format!("cargo-heft-{pid}"));
        assert!(!scratch.exists(), "left behind: {}", scratch.display());
    }
}

#[test]
fn the_workspace_target_dir_is_the_default() {
    let dir = TempDir::new("default-target");
    write_file(
        dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"tiny\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write_file(dir.path(), "src/main.rs", b"fn main() {}\n");
    stdout(
        Command::new(env!("CARGO"))
            .args(["build", "--quiet"])
            .current_dir(dir.path())
            .env_remove("CARGO_TARGET_DIR"),
    );

    let out = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
        .arg("--json")
        .current_dir(dir.path())
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("run cargo-heft");
    assert!(
        out.status.success(),
        "cargo-heft failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_consistent(&report);
    assert_eq!(
        Path::new(report["target_dir"].as_str().unwrap()),
        dir.join("target"),
        "the default must be the workspace target directory from cargo metadata"
    );
    assert!(report["by_kind"]["binary"].as_u64().unwrap() > 0);
}

#[test]
fn a_missing_target_dir_is_explained() {
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("never-built");
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
        .arg("--target-dir")
        .arg(&missing)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("does not exist"), "{stderr}");
    assert!(stderr.contains("cargo build"), "{stderr}");
    assert!(!stderr.contains("os error"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_an_error_not_a_panic() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    for extra in [&[][..], &["--json"][..]] {
        let out = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .arg("--target-dir")
            .arg(OsStr::from_bytes(b"no-such-\xff"))
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("nothing to measure"), "{stderr}");
        assert!(!stderr.contains("panicked"), "{stderr}");
    }
    // The flag itself can be garbage too.
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
        .arg(OsStr::from_bytes(b"--\xff"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
}

#[test]
fn a_reader_that_leaves_early_is_not_a_panic() {
    use std::io::Read;
    use std::process::Stdio;
    // Enough entries that the output does not fit in a pipe buffer.
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("many-crates");
    let deps = target.join("debug/deps");
    std::fs::create_dir_all(&deps).unwrap();
    for i in 0..6000 {
        std::fs::write(
            deps.join(format!("libcrate{i:05}-0123456789abcdef.rmeta")),
            b"",
        )
        .unwrap();
    }
    for mode in [&["--json"][..], &["--top", "100000"][..]] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .arg("--target-dir")
            .arg(&target)
            .args(mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut first = [0u8; 64];
        child.stdout.take().unwrap().read_exact(&mut first).unwrap(); // then hang up
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{mode:?}: {:?} {stderr}", out.status);
        assert!(stderr.is_empty(), "{stderr}");
    }
}

#[test]
fn a_subdirectory_of_a_target_keeps_its_meaning() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixture-target-subdirs");
    let _ = std::fs::remove_dir_all(&target);
    build_fixture(&target, &[]);
    stdout(
        Command::new(env!("CARGO"))
            .args(["doc", "--no-deps"])
            .current_dir(fixture())
            .env("CARGO_TARGET_DIR", &target),
    );
    let measure = |dir: &Path| report(&repo(), &["--target-dir", dir.to_str().unwrap()]);
    let whole = measure(&target);
    let kinds = |v: &serde_json::Value, k: &str| v["by_kind"][k].as_u64().unwrap_or(0);

    // The debug profile alone: named as such, and the same bytes as in the whole.
    let debug = measure(&target.join("debug"));
    assert_consistent(&debug);
    let profiles: Vec<&String> = debug["by_profile"].as_object().unwrap().keys().collect();
    assert_eq!(profiles, ["debug"]);
    assert_eq!(debug["total_bytes"], whole["by_profile"]["debug"]);
    for kind in [
        "rlib",
        "rmeta",
        "build script output",
        "build script executable",
    ] {
        assert_eq!(kinds(&debug, kind), kinds(&whole, kind), "{kind}");
    }

    // The docs alone: all of it is documentation, as in the whole.
    let doc = measure(&target.join("doc"));
    assert_consistent(&doc);
    let doc_kinds: Vec<&String> = doc["by_kind"].as_object().unwrap().keys().collect();
    assert_eq!(doc_kinds, ["directories", "doc"]);
    assert_eq!(kinds(&doc, "doc"), kinds(&whole, "doc"));
    assert!(kinds(&whole, "doc") > 0);

    // A build script's own dep-info is dep-info, not part of the executable.
    // On Linux the classic layout hard-links the script under two names, so
    // count each inode once, as the tool does.
    let mut seen = std::collections::HashSet::new();
    let exe: u64 = std::fs::read_dir(target.join("debug/build"))
        .unwrap()
        .flatten()
        .flat_map(|unit| {
            std::fs::read_dir(unit.path())
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter(|f| {
            let n = f.file_name().to_string_lossy().into_owned();
            (n.starts_with("build_script_") || n.starts_with("build-script-")) && !n.ends_with(".d")
                || n == "build_script_build" // the v2 layout, in out/
        })
        .filter(|f| {
            use std::os::unix::fs::MetadataExt;
            let md = f.metadata().unwrap();
            seen.insert((md.dev(), md.ino()))
        })
        .map(|f| disk_bytes(&f.path()))
        .sum();
    if exe > 0 {
        assert_eq!(kinds(&whole, "build script executable"), exe);
    }
}

#[test]
fn a_missing_profile_suggests_building_that_profile() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixture-target-hint");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join("CACHEDIR.TAG"),
        b"Signature: 8a477f597d28d172789f06886806bc55",
    )
    .unwrap();
    for (dir, hint) in [
        ("release", "`cargo build --release`"),
        ("bench", "`cargo build --profile bench`"),
        ("debug", "`cargo build`"),
    ] {
        let out = run(&["--target-dir", target.join(dir).to_str().unwrap()]);
        out.assert_failed();
        assert!(out.stderr.contains(hint), "{dir}: {}", out.stderr);
    }
}
