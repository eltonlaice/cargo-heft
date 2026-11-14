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
            .env("CARGO_TARGET_DIR", &target),
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
    for kind in ["rlib", "rmeta", "incremental", "binary"] {
        assert!(
            kinds.get(kind).is_some_and(|b| b.as_u64().unwrap() > 0),
            "no {kind} in {kinds:?}"
        );
    }
    assert!(v["by_crate"]["dep"].as_u64().unwrap() > 0);
}
