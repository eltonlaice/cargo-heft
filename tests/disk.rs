//! `cargo heft disk` on Cargo and rustup homes these tests lay out themselves.

mod common;

use std::fs;
use std::path::Path;

use common::*;

fn file(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![7u8; bytes]).unwrap();
}

/// The usual layout of both homes, with a default and an older toolchain.
fn homes(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let (cargo, rustup) = (root.join("cargo"), root.join("rustup"));
    file(&cargo.join("registry/index/idx"), 10_000);
    file(&cargo.join("registry/cache/a.crate"), 20_000);
    file(&cargo.join("registry/src/a/lib.rs"), 30_000);
    file(&cargo.join("git/db/repo/pack"), 40_000);
    file(&cargo.join("git/checkouts/repo/x.rs"), 50_000);
    file(&cargo.join("bin/tool"), 60_000);
    file(&cargo.join("credentials.toml"), 100);
    file(&rustup.join("toolchains/stable-x/lib/libstd.rlib"), 70_000);
    file(&rustup.join("toolchains/1.80.0-x/lib/libstd.rlib"), 80_000);
    file(&rustup.join("downloads/partial"), 1_000);
    fs::write(
        rustup.join("settings.toml"),
        "version = \"12\"\ndefault_toolchain = \"stable-x\"\n",
    )
    .unwrap();
    (cargo, rustup)
}

fn disk(cargo: &Path, rustup: &Path) -> Outcome {
    disk_in(&repo(), cargo, rustup, &[])
}

/// `cargo heft disk` in `dir`. The rustup proxy sets RUSTUP_TOOLCHAIN for the
/// tests themselves, so it is removed unless the test sets it.
fn disk_in(dir: &Path, cargo: &Path, rustup: &Path, envs: &[(&str, &str)]) -> Outcome {
    let (c, r) = (cargo.to_str().unwrap(), rustup.to_str().unwrap());
    let mut all = vec![("CARGO_HOME", c), ("RUSTUP_HOME", r)];
    all.extend_from_slice(envs);
    let remove: &[&str] = match envs.iter().any(|(k, _)| *k == "RUSTUP_TOOLCHAIN") {
        true => &[],
        false => &["RUSTUP_TOOLCHAIN"],
    };
    run_with(dir, &["disk"], &all, remove)
}

/// The tool's own rounding: decimal units, as in the report.
fn human_bytes(bytes: u64) -> String {
    let kb = bytes as f64 / 1e3;
    match bytes {
        b if b < 1000 => format!("{b} B"),
        _ if kb < 999.95 => format!("{kb:.1} kB"),
        _ if kb / 1e3 < 999.95 => format!("{:.1} MB", kb / 1e3),
        _ => format!("{:.2} GB", kb / 1e6),
    }
}

/// The row of `part`: its class and command, if any.
fn row<'a>(out: &'a str, part: &str) -> Vec<&'a str> {
    let line = out
        .lines()
        .find(|l| l.split_whitespace().next() == Some(part))
        .unwrap_or_else(|| panic!("no row for {part} in:\n{out}"));
    line.split_whitespace().skip(3).collect()
}

fn du_bytes(path: &Path) -> u64 {
    let out = std::process::Command::new("du")
        .arg("-sk")
        .arg(path)
        .output()
        .unwrap();
    let kib: u64 = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    kib * 1024
}

#[test]
fn each_part_is_classified_with_the_command_that_removes_it() {
    let t = TempDir::new("disk-usual");
    let tmp = t.path();
    let (cargo, rustup) = homes(tmp);
    let out = disk(&cargo, &rustup);
    out.assert_ok();
    let gc = ["cargo", "+nightly", "-Zgc", "clean", "gc"];
    for (part, class) in [
        ("registry/index", "download"),
        ("registry/cache", "download"),
        ("registry/src", "rebuilt"),
        ("git/db", "download"),
        ("git/checkouts", "rebuilt"),
    ] {
        let r = row(&out.stdout, part);
        assert_eq!(r[0], class, "{part}");
        assert_eq!(r[1..], gc, "{part}");
    }
    assert_eq!(row(&out.stdout, "bin"), ["keep"]);
    // Credentials and settings are "other", never something to remove.
    assert_eq!(row(&out.stdout, "other"), ["keep"]);
    assert_eq!(
        row(&out.stdout, "toolchains/stable-x"),
        ["keep", "(in", "use:", "default)"],
        "the default toolchain"
    );
    assert_eq!(
        row(&out.stdout, "toolchains/1.80.0-x"),
        ["download", "rustup", "toolchain", "uninstall", "1.80.0-x"]
    );
    assert!(out.stdout.contains("Nothing was removed"));
    assert!(
        cargo.join("registry/src/a/lib.rs").exists() && rustup.join("toolchains/1.80.0-x").exists()
    );
}

#[test]
fn the_total_of_each_home_matches_du() {
    let t = TempDir::new("disk-du");
    let tmp = t.path();
    let (cargo, rustup) = homes(tmp);
    let out = disk(&cargo, &rustup);
    out.assert_ok();
    for (title, path) in [("Cargo home", &cargo), ("rustup home", &rustup)] {
        let line = out.stdout.lines().find(|l| l.starts_with(title)).unwrap();
        let want = format!("({}, ", human_bytes(du_bytes(path)));
        assert!(line.contains(&want), "{line} vs du {want}");
    }
}

#[test]
fn a_missing_home_is_reported_not_an_error() {
    let t = TempDir::new("disk-missing");
    let tmp = t.path();
    let (cargo, _) = homes(tmp);
    let out = disk(&cargo, &tmp.join("no-such-rustup"));
    out.assert_ok();
    assert!(
        out.stdout.contains("rustup home: ") && out.stdout.contains("(not found)"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("registry/src"));
}

#[test]
fn a_symlink_is_counted_not_followed() {
    let t = TempDir::new("disk-symlink");
    let tmp = t.path();
    let (cargo, rustup) = homes(tmp);
    let before = disk(&cargo, &rustup);
    file(&tmp.join("elsewhere/huge"), 5_000_000);
    std::os::unix::fs::symlink(tmp.join("elsewhere"), cargo.join("registry/src/link")).unwrap();
    let after = disk(&cargo, &rustup);
    after.assert_ok();
    let total = |o: &Outcome| {
        o.stdout
            .lines()
            .find(|l| l.starts_with("Total:"))
            .unwrap()
            .to_string()
    };
    // The link adds a few bytes at most, never the 5 MB behind it.
    assert!(
        !total(&after).contains("MB"),
        "{} -> {}",
        total(&before),
        total(&after)
    );
}

#[test]
fn an_unreadable_directory_is_named_and_the_rest_measured() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempDir::new("disk-unreadable");
    let tmp = t.path();
    let (cargo, rustup) = homes(tmp);
    let locked = cargo.join("registry/src/a");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let out = disk(&cargo, &rustup);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    out.assert_ok();
    if fs::read_dir(&locked).is_ok() && unsafe_running_as_root() {
        return; // root reads everything; nothing to check
    }
    assert!(out.stderr.contains("could not be read"), "{}", out.stderr);
    assert!(out.stderr.contains("registry/src/a"), "{}", out.stderr);
    assert!(out.stdout.contains("registry/cache"));
}

#[test]
fn an_unknown_layout_is_measured_and_nothing_recommended() {
    let t = TempDir::new("disk-unknown");
    let tmp = t.path();
    let cargo = tmp.join("cargo");
    file(&cargo.join("something/new/file"), 30_000);
    let out = disk(&cargo, &tmp.join("rustup"));
    out.assert_ok();
    assert_eq!(row(&out.stdout, "other"), ["keep"]);
    assert!(
        !out.stdout.contains("-Zgc") && !out.stdout.contains("uninstall"),
        "{}",
        out.stdout
    );
}

#[test]
fn disk_takes_no_arguments() {
    let out = run(&["disk", "--json"]);
    out.assert_failed();
    assert!(
        out.stderr
            .contains("unknown argument `--json` for `cargo heft disk`"),
        "{}",
        out.stderr
    );
}

fn unsafe_running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| o.stdout.starts_with(b"0"))
}

/// The row of the older toolchain when something may be using it.
fn old_toolchain_row(dir: &Path, rustup_extra: &str, envs: &[(&str, &str)]) -> String {
    let t = TempDir::new("disk-in-use");
    let (cargo, rustup) = homes(t.path());
    if !rustup_extra.is_empty() {
        let settings = rustup.join("settings.toml");
        let text = fs::read_to_string(&settings).unwrap() + rustup_extra;
        fs::write(settings, text).unwrap();
    }
    let out = disk_in(dir, &cargo, &rustup, envs);
    out.assert_ok();
    row(&out.stdout, "toolchains/1.80.0-x").join(" ")
}

#[test]
fn a_toolchain_nothing_uses_is_offered_for_uninstall() {
    let t = TempDir::new("disk-unused");
    assert_eq!(
        old_toolchain_row(t.path(), "", &[]),
        "download rustup toolchain uninstall 1.80.0-x"
    );
}

#[test]
fn a_toolchain_pinned_here_or_above_is_kept() {
    let t = TempDir::new("disk-pinned");
    let project = t.join("project/src");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        t.join("project/rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.80.0\"\n",
    )
    .unwrap();
    let r = old_toolchain_row(&project, "", &[]);
    assert!(r.starts_with("keep (in use: pinned by "), "{r}");
    assert!(r.ends_with("rust-toolchain.toml)"), "{r}");

    // The old format: only the toolchain name.
    let legacy = TempDir::new("disk-pinned-legacy");
    fs::write(legacy.join("rust-toolchain"), "1.80.0\n").unwrap();
    let r = old_toolchain_row(legacy.path(), "", &[]);
    assert!(r.starts_with("keep (in use: pinned by "), "{r}");
}

#[test]
fn a_rustup_override_is_kept() {
    let t = TempDir::new("disk-override");
    let r = old_toolchain_row(t.path(), "\n[overrides]\n\"/work/old\" = \"1.80.0\"\n", &[]);
    assert_eq!(r, "keep (in use: override for /work/old)");
}

#[test]
fn the_toolchain_in_rustup_toolchain_is_kept() {
    let t = TempDir::new("disk-env");
    let r = old_toolchain_row(t.path(), "", &[("RUSTUP_TOOLCHAIN", "1.80.0")]);
    assert_eq!(r, "keep (in use: RUSTUP_TOOLCHAIN)");
}

#[test]
fn a_name_is_not_matched_by_a_longer_version() {
    // 1.8 must not keep 1.80.0: the name has to end where the triple starts.
    let t = TempDir::new("disk-prefix");
    let r = old_toolchain_row(t.path(), "", &[("RUSTUP_TOOLCHAIN", "1.8")]);
    assert!(r.starts_with("download"), "{r}");
}
