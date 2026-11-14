//! The total must match `du`, and a real build must be recognised.
use std::path::Path;
use std::process::Command;

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

#[test]
fn total_matches_du_on_a_real_build() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture");
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixture-target");
    let _ = std::fs::remove_dir_all(&target);
    stdout(
        Command::new(env!("CARGO"))
            .arg("build")
            .current_dir(&fixture)
            .env("CARGO_TARGET_DIR", &target)
            // Cargo 1.100 turns incremental off when `CI` is set.
            .env("CARGO_INCREMENTAL", "1"),
    );

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
    assert!(v["duplicated_metadata_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn what_if_measures_each_setting() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture");
    let json = stdout(
        Command::new(env!("CARGO_BIN_EXE_cargo-heft"))
            .args(["--what-if", "--json"])
            .current_dir(&fixture)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CI"),
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let size = |name: &str| {
        v["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap_or_else(|| panic!("no scenario {name}"))["total_bytes"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(v["scenarios"].as_array().unwrap().len(), 5);
    assert!(size("debug = false") < size("as configured"));
    // `<=`: where Cargo already builds without incremental, nothing changes.
    assert!(size("incremental = false") <= size("as configured"));
    assert!(size("line-tables-only, no incremental") <= size("debug = \"line-tables-only\""));
    assert!(size("debug = \"line-tables-only\"") < size("as configured"));
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
