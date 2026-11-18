//! End-to-end test for the build-dir v2 layout that Cargo 1.100 introduced.
//!
//! The layout is the reason this tool exists, so it is worth testing against a
//! real Cargo, not only against hand-written directories. Cargo 1.100 is not
//! necessarily installed: when `rustup run 1.100.0` is unavailable the test
//! prints why and passes, so the suite still works on machines with a single
//! stable toolchain. The `layout-v2` CI job installs 1.100.0 and sets
//! `HEFT_REQUIRE_CARGO_1_100`, so there the test cannot skip.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::*;

fn is_hash(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True when `build/<pkg>/<hash>/out` exists somewhere below `target/debug`.
fn has_v2_unit(build: &Path) -> bool {
    let Ok(packages) = fs::read_dir(build) else {
        return false;
    };
    for package in packages.flatten() {
        let Ok(units) = fs::read_dir(package.path()) else {
            continue;
        };
        for unit in units.flatten() {
            let Some(name) = unit.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if is_hash(&name) && unit.path().join("out").is_dir() {
                return true;
            }
        }
    }
    false
}

/// True when some unit has a `run/` directory (a build script execution unit).
fn has_run_unit(build: &Path) -> bool {
    let Ok(packages) = fs::read_dir(build) else {
        return false;
    };
    for package in packages.flatten() {
        let Ok(units) = fs::read_dir(package.path()) else {
            continue;
        };
        for unit in units.flatten() {
            let Some(name) = unit.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if is_hash(&name) && unit.path().join("run").is_dir() {
                return true;
            }
        }
    }
    false
}

fn cargo_1_100() -> Option<Command> {
    let probe = Command::new("rustup")
        .args(["run", "1.100.0", "cargo", "--version"])
        .output()
        .ok()?;
    if !probe.status.success() {
        return None;
    }
    let mut cargo = Command::new("rustup");
    cargo.args(["run", "1.100.0", "cargo"]);
    Some(cargo)
}

#[test]
fn measures_a_cargo_1_100_build_dir_v2_layout() {
    let Some(mut cargo) = cargo_1_100() else {
        // The CI job sets this, so a missing toolchain fails there instead of
        // passing without testing anything.
        assert!(
            std::env::var_os("HEFT_REQUIRE_CARGO_1_100").is_none(),
            "HEFT_REQUIRE_CARGO_1_100 is set but `rustup run 1.100.0 cargo` is unavailable"
        );
        eprintln!(
            "skipping: `rustup run 1.100.0 cargo` is unavailable; \
             install the 1.100.0 toolchain to exercise the v2 layout"
        );
        return;
    };

    let dir = TempDir::new("cargo-1-100");
    let target = dir.join("target");
    let build = cargo
        .current_dir(fixture())
        .arg("build")
        .arg("--quiet")
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "1")
        .env_remove("CI")
        .output()
        .expect("run cargo 1.100.0");
    assert!(
        build.status.success(),
        "cargo 1.100 build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let debug = target.join("debug");
    let build_dir = debug.join("build");
    assert!(
        has_v2_unit(&build_dir),
        "Cargo 1.100 did not produce build/<pkg>/<hash>/out under {}",
        build_dir.display()
    );
    assert!(
        has_run_unit(&build_dir),
        "Cargo 1.100 did not produce a run/ unit under {}",
        build_dir.display()
    );

    let out = run_in(
        &fixture(),
        &["--json", "--target-dir", target.to_str().unwrap()],
    );
    let report = out.json();
    assert_consistent(&report);
    let total = report["total_bytes"].as_u64().unwrap();
    assert!(total > 0);

    let kinds = report["by_kind"].as_object().unwrap();
    for kind in [
        "rlib",
        "rmeta",
        "object file",
        "fingerprint",
        "build script executable",
        "build script output",
        "binary",
    ] {
        assert!(
            kinds.get(kind).is_some_and(|b| b.as_u64().unwrap() > 0),
            "{kind} missing from a v2 build: {kinds:?}"
        );
    }

    let crates = report["by_crate"].as_object().unwrap();
    for name in ["dep", "app"] {
        assert!(
            crates.get(name).is_some_and(|b| b.as_u64().unwrap() > 0),
            "crate {name} missing from {crates:?}"
        );
    }

    let content = report["by_content"].as_object().unwrap();
    assert!(content["metadata (inside rlib/dylib)"].as_u64().unwrap() > 0);
    assert!(content["metadata (.rmeta files)"].as_u64().unwrap() > 0);
    assert!(content["code, data and symbols"].as_u64().unwrap() > 0);
    assert!(
        report["duplicated_metadata_bytes"].as_u64().unwrap() > 0,
        "the fixture has a .rmeta next to the matching .rlib"
    );

    #[cfg(unix)]
    assert_matches_du(total, &target);
}
