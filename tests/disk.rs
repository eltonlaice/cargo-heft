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
#[cfg(unix)]
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

#[cfg(unix)]
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
    // Progress goes to a terminal only: piped, stderr stays empty.
    assert!(out.stderr.is_empty(), "{}", out.stderr);
    assert!(
        cargo.join("registry/src/a/lib.rs").exists() && rustup.join("toolchains/1.80.0-x").exists()
    );
}

// `du` on Windows is an emulation (Git Bash) that counts directories its own
// way, so these comparisons run on unix only.
#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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
fn disk_rejects_an_unknown_argument() {
    let out = run(&["disk", "--top", "3"]);
    out.assert_failed();
    assert!(
        out.stderr
            .contains("unknown argument `--top` for `cargo heft disk`"),
        "{}",
        out.stderr
    );
}

#[test]
fn disk_json_has_the_same_numbers_and_says_why_a_part_is_kept() {
    let t = TempDir::new("disk-json");
    let (cargo, rustup) = homes(t.path());
    let (c, r) = (cargo.to_str().unwrap(), rustup.to_str().unwrap());
    let out = run_with(
        &repo(),
        &["disk", "--json"],
        &[("CARGO_HOME", c), ("RUSTUP_HOME", r)],
        &["RUSTUP_TOOLCHAIN"],
    );
    let v = out.json();
    assert_eq!(v["schema_version"], 1);
    let homes = v["homes"].as_array().unwrap();
    assert_eq!(homes[0]["name"], "cargo");
    assert_eq!(homes[1]["name"], "rustup");
    let sum: u64 = homes
        .iter()
        .map(|h| h["total_bytes"].as_u64().unwrap())
        .sum();
    assert_eq!(v["total_bytes"].as_u64().unwrap(), sum);
    for h in homes {
        let parts: u64 = h["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["bytes"].as_u64().unwrap())
            .sum();
        assert_eq!(parts, h["total_bytes"].as_u64().unwrap(), "{h}");
        #[cfg(unix)]
        assert_eq!(
            h["total_bytes"].as_u64().unwrap(),
            du_bytes(Path::new(h["path"].as_str().unwrap()))
        );
    }
    let part = |home: usize, name: &str| {
        homes[home]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()
            .clone()
    };
    let src = part(0, "registry/src");
    assert_eq!(src["class"], "rebuilt");
    assert_eq!(src["command"], "cargo +nightly -Zgc clean gc");
    let stable = part(1, "toolchains/stable-x");
    assert_eq!(stable["class"], "keep");
    assert!(stable["command"].is_null());
    assert_eq!(stable["kept_because"], "default");
    assert_eq!(
        part(1, "toolchains/1.80.0-x")["command"],
        "rustup toolchain uninstall 1.80.0-x"
    );
    assert!(v["unreadable"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
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

#[test]
fn a_channel_name_does_not_keep_dated_toolchains() {
    // `channel = "nightly"` means today's nightly, not every dated one: the
    // dated ones are what someone running this wants to find.
    let t = TempDir::new("disk-dated");
    let (cargo, rustup) = homes(t.path());
    file(&rustup.join("toolchains/nightly-x/lib/libstd.rlib"), 10_000);
    file(
        &rustup.join("toolchains/nightly-2026-11-01-x/lib/libstd.rlib"),
        10_000,
    );
    fs::write(t.join("rust-toolchain"), "nightly\n").unwrap();
    let out = disk_in(t.path(), &cargo, &rustup, &[]);
    out.assert_ok();
    assert!(
        row(&out.stdout, "toolchains/nightly-x")[0] == "keep",
        "{}",
        out.stdout
    );
    assert_eq!(
        row(&out.stdout, "toolchains/nightly-2026-11-01-x").join(" "),
        "download rustup toolchain uninstall nightly-2026-11-01-x"
    );
}

/// A package at `dir` with a source file, so `cargo metadata` accepts it.
fn package(dir: &Path, name: &str) {
    file(&dir.join("src/lib.rs"), 0);
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
}

/// A target directory as Cargo marks it, with `bytes` of build output.
fn built(target: &Path, bytes: usize) {
    file(&target.join("CACHEDIR.TAG"), 43);
    file(&target.join("debug/deps/libx.rlib"), bytes);
}

/// `cargo heft disk` with `args`, the homes in `tmp` and no target directory
/// set from outside unless `envs` sets one.
fn disk_projects(tmp: &Path, args: &[&str], envs: &[(&str, &str)]) -> Outcome {
    let (cargo, rustup) = homes(tmp);
    let (c, r) = (cargo.to_str().unwrap(), rustup.to_str().unwrap());
    let mut all = vec![("CARGO_HOME", c), ("RUSTUP_HOME", r)];
    all.extend_from_slice(envs);
    let mut remove = vec!["RUSTUP_TOOLCHAIN"];
    if !envs.iter().any(|(k, _)| *k == "CARGO_TARGET_DIR") {
        remove.push("CARGO_TARGET_DIR");
    }
    let mut a = vec!["disk"];
    a.extend_from_slice(args);
    run_with(tmp, &a, &all, &remove)
}

/// `a/b` as the report prints it on this system (`a\\b` on Windows).
fn native(rel: &str) -> String {
    Path::new(rel)
        .iter()
        .collect::<std::path::PathBuf>()
        .display()
        .to_string()
}

/// The rows under "Target directories": name, then the rest split on spaces.
fn project_rows(out: &str) -> Vec<Vec<String>> {
    out.lines()
        .skip_while(|l| !l.starts_with("Target directories"))
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| l.split_whitespace().map(String::from).collect())
        .collect()
}

#[test]
fn projects_lists_each_workspace_target_once() {
    let t = TempDir::new("disk-projects");
    let code = t.join("code");
    // A package, a workspace of two members, and a package whose
    // .cargo/config.toml moves its target directory.
    package(&code.join("a"), "a");
    built(&code.join("a/target"), 30_000);
    fs::create_dir_all(code.join("ws")).unwrap();
    fs::write(
        code.join("ws/Cargo.toml"),
        "[workspace]\nmembers = [\"m1\", \"m2\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    package(&code.join("ws/m1"), "m1");
    package(&code.join("ws/m2"), "m2");
    built(&code.join("ws/target"), 50_000);
    package(&code.join("c"), "c");
    fs::create_dir_all(code.join("c/.cargo")).unwrap();
    fs::write(
        code.join("c/.cargo/config.toml"),
        "[build]\ntarget-dir = \"out\"\n",
    )
    .unwrap();
    built(&code.join("c/out"), 20_000);
    // A Cargo.toml inside a target directory is build output, not a project.
    package(&code.join("a/target/package/a-0.1.0"), "a");
    // A package that was never built has nothing to list.
    package(&code.join("new"), "new");

    let out = disk_projects(t.path(), &["--projects", code.to_str().unwrap()], &[]);
    out.assert_ok();
    let rows = project_rows(&out.stdout);
    let names: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
    let want_names: Vec<String> = ["ws/target", "a/target", "c/out"].map(native).into();
    assert_eq!(names, want_names, "{}", out.stdout);
    for r in &rows {
        assert_eq!(r[3..6], ["rebuilt", "cargo", "clean"], "{r:?}");
    }
    // The workspace's own manifest, as Cargo names it (a real path).
    assert!(
        rows[0].last().unwrap().ends_with(&native("ws/Cargo.toml")),
        "{:?}",
        rows[0]
    );
    // The section total is what du gives for the three directories.
    #[cfg(unix)]
    {
        let want: u64 = ["a/target", "ws/target", "c/out"]
            .iter()
            .map(|d| du_bytes(&code.join(d)))
            .sum();
        let header = out
            .stdout
            .lines()
            .find(|l| l.starts_with("Target directories"))
            .unwrap();
        assert!(
            header.contains(&format!("({}, ", human_bytes(want))),
            "{header}"
        );
    }
    // Read only.
    assert!(code.join("a/target/debug/deps/libx.rlib").exists());
}

#[test]
fn a_shared_target_directory_is_counted_once() {
    let t = TempDir::new("disk-projects-shared");
    let code = t.join("code");
    package(&code.join("a"), "a");
    package(&code.join("b"), "b");
    let shared = t.join("shared");
    built(&shared, 40_000);
    let out = disk_projects(
        t.path(),
        &["--projects", code.to_str().unwrap()],
        &[("CARGO_TARGET_DIR", shared.to_str().unwrap())],
    );
    out.assert_ok();
    let rows = project_rows(&out.stdout);
    assert_eq!(rows.len(), 1, "{}", out.stdout);
    assert_eq!(rows[0][0], shared.canonicalize().unwrap().to_str().unwrap());
}

#[test]
fn projects_in_json_and_without_a_directory() {
    let t = TempDir::new("disk-projects-json");
    let code = t.join("code");
    package(&code.join("a"), "a");
    built(&code.join("a/target"), 30_000);
    let arg = format!("--projects={}", code.display());
    let v = disk_projects(t.path(), &[&arg, "--json"], &[])
        .assert_ok()
        .json();
    let homes = v["homes"].as_array().unwrap();
    let names: Vec<&str> = homes.iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["cargo", "rustup", "projects"]);
    let p = &homes[2];
    assert_eq!(p["parts"][0]["name"], native("a/target"));
    assert_eq!(p["parts"][0]["class"], "rebuilt");
    let sum: u64 = homes
        .iter()
        .map(|h| h["total_bytes"].as_u64().unwrap())
        .sum();
    assert_eq!(v["total_bytes"].as_u64().unwrap(), sum);

    let empty = t.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let out = disk_projects(t.path(), &["--projects", empty.to_str().unwrap()], &[]);
    out.assert_ok();
    assert!(
        out.stdout.contains("no built Cargo workspace found"),
        "{}",
        out.stdout
    );

    let out = disk_projects(t.path(), &["--projects"], &[]);
    out.assert_failed();
    assert!(
        out.stderr.contains("--projects needs a directory"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_pinned_toolchain_that_is_missing_is_not_installed() {
    let t = TempDir::new("disk-projects-pin");
    let code = t.join("code");
    package(&code.join("p"), "p");
    fs::write(
        code.join("p/rust-toolchain.toml"),
        "[toolchain]\nchannel = \"nightly-2025-01-02\"\n",
    )
    .unwrap();
    built(&code.join("p/target"), 10_000);
    // Through the rustup proxy, as when the binary is run on its own.
    let out = run_with(
        t.path(),
        &["disk", "--projects", code.to_str().unwrap()],
        &[
            ("CARGO_HOME", t.join("cargo").to_str().unwrap()),
            ("RUSTUP_HOME", t.join("rustup").to_str().unwrap()),
        ],
        &["CARGO", "RUSTUP_TOOLCHAIN", "CARGO_TARGET_DIR"],
    );
    out.assert_ok();
    assert_eq!(
        project_rows(&out.stdout)[0][0],
        native("p/target"),
        "{}",
        out.stdout
    );
    assert!(
        !t.join("rustup/toolchains").exists(),
        "a toolchain was installed"
    );
}
