//! Measuring synthetic target directories.
//!
//! Building a real workspace for every classification rule would make the
//! suite slow. These tests write the files Cargo and rustc would leave behind
//! and check how `cargo-heft` accounts for them. The end-to-end tests then
//! confirm the same rules on a real build.

mod common;

use std::path::Path;

use common::*;

/// A target directory in the classic (Cargo up to 1.99) layout, with one file
/// of every kind the tool knows about.
fn classic_tree(root: &Path) {
    let deps = "debug/deps";
    write_sized(
        root,
        &format!("{deps}/libcore-0123456789abcdef.rlib"),
        1,
        b'r',
    );
    write_sized(
        root,
        &format!("{deps}/libserde-0123456789abcdef.rmeta"),
        1,
        b'm',
    );
    write_sized(root, &format!("{deps}/app-0123456789abcdef.d"), 1, b'd');
    write_sized(
        root,
        &format!("{deps}/app-0123456789abcdef.1x.rcgu.o"),
        1,
        b'o',
    );
    write_sized(
        root,
        &format!("{deps}/libfoo-0123456789abcdef.dylib"),
        1,
        b'y',
    );
    write_sized(root, &format!("{deps}/libfoo-0123456789abcdef.so"), 1, b's');
    write_sized(root, &format!("{deps}/foo-0123456789abcdef.dll"), 1, b'l');
    write_sized(root, &format!("{deps}/app-0123456789abcdef.exe"), 1, b'e');
    let runner = write_sized(root, &format!("{deps}/runner-0123456789abcdef"), 1, b'x');
    set_executable(&runner);
    write_sized(root, &format!("{deps}/split-0123456789abcdef.dwo"), 1, b'w');
    write_sized(root, &format!("{deps}/split-0123456789abcdef.dwp"), 1, b'p');
    write_sized(
        root,
        &format!("{deps}/app-0123456789abcdef.dSYM/Contents/Resources/DWARF/app"),
        1,
        b'g',
    );
    let script = write_sized(
        root,
        "debug/build/serde-0123456789abcdef/build_script_build-0123456789abcdef",
        1,
        b'b',
    );
    set_executable(&script);
    write_sized(
        root,
        "debug/build/serde-0123456789abcdef/out/generated.rs",
        1,
        b'g',
    );
    write_sized(
        root,
        "debug/.fingerprint/serde-0123456789abcdef/lib-serde.json",
        1,
        b'f',
    );
    write_sized(
        root,
        "debug/incremental/app-2k1x9/s-a-b/dep-graph.bin",
        1,
        b'i',
    );
    write_sized(root, "doc/foo/index.html", 1, b'h');
    write_sized(root, "package/foo.crate", 1, b'c');
    write_sized(
        root,
        &format!("{deps}/unknown-0123456789abcdef.widget"),
        1,
        b'u',
    );
}

#[test]
fn classifies_every_kind_in_the_classic_layout() {
    let dir = TempDir::new("classic");
    classic_tree(dir.path());
    let report = report(dir.path(), &["--target-dir", dir.path().to_str().unwrap()]);
    assert_consistent(&report);

    let kinds = report["by_kind"].as_object().unwrap();
    for kind in [
        "rlib",
        "rmeta",
        "dep-info",
        "object file",
        "dylib",
        "binary",
        "debuginfo (split DWARF)",
        "debuginfo (dSYM)",
        "build script executable",
        "build script output",
        "fingerprint",
        "incremental",
        "doc",
        "package",
        "other",
    ] {
        let bytes = kinds[kind].as_u64().unwrap();
        assert!(bytes > 0, "{kind} was not measured: {kinds:?}");
    }
    assert!(kinds.contains_key("directories"));

    let crates = report["by_crate"].as_object().unwrap();
    for name in ["core", "serde", "app", "foo", "runner", "split", "unknown"] {
        assert!(
            crates.get(name).is_some_and(|b| b.as_u64().unwrap() > 0),
            "crate {name} missing from {crates:?}"
        );
    }
    assert!(!crates.contains_key("doc"));
    assert!(!crates.contains_key("package"));

    let content = report["by_content"].as_object().unwrap();
    for part in [
        "code, data and symbols",
        "metadata (.rmeta files)",
        "debuginfo",
        "incremental cache",
        "other",
    ] {
        assert!(
            content.get(part).is_some_and(|b| b.as_u64().unwrap() > 0),
            "content part {part} missing from {content:?}"
        );
    }
    assert_eq!(
        report["duplicated_metadata_bytes"], 0,
        "a plain file named .rlib holds no archive metadata"
    );

    #[cfg(unix)]
    assert_matches_du(report["total_bytes"].as_u64().unwrap(), dir.path());
}

#[test]
fn classifies_the_build_dir_v2_layout() {
    let dir = TempDir::new("v2");
    let root = dir.path();
    // A library unit: compiler output in out/.
    write_sized(
        root,
        "debug/build/dep/1111111111111111/out/libdep-0123456789abcdef.rlib",
        1,
        b'r',
    );
    write_sized(
        root,
        "debug/build/dep/1111111111111111/out/libdep-0123456789abcdef.rmeta",
        1,
        b'm',
    );
    write_sized(
        root,
        "debug/build/dep/1111111111111111/out/dep-0123456789abcdef.1x.rcgu.o",
        1,
        b'o',
    );
    write_sized(
        root,
        "debug/build/dep/1111111111111111/out/dep-0123456789abcdef.d",
        1,
        b'd',
    );
    write_sized(
        root,
        "debug/build/dep/1111111111111111/fingerprint/lib-dep.json",
        1,
        b'f',
    );
    // The unit that compiles the build script: its executable in out/, no run/.
    let script = write_sized(
        root,
        "debug/build/dep/2222222222222222/out/build_script_build",
        1,
        b'b',
    );
    set_executable(&script);
    write_sized(
        root,
        "debug/build/dep/2222222222222222/out/build_script_build.d",
        1,
        b'd',
    );
    // The unit that runs the build script: run/ marks its unit, and out/ holds
    // the generated files.
    write_sized(
        root,
        "debug/build/dep/3333333333333333/out/generated.rs",
        1,
        b'g',
    );
    write_sized(root, "debug/build/dep/3333333333333333/run/stdout", 1, b's');
    write_sized(root, "debug/build/dep/3333333333333333/run/stderr", 1, b'e');
    // A binary unit.
    let binary = write_sized(root, "debug/build/app/4444444444444444/out/app", 1, b'x');
    set_executable(&binary);

    let out = run_in(root, &["--json", "--target-dir", root.to_str().unwrap()]);
    let report = out.json();
    assert_consistent(&report);

    let kinds = report["by_kind"].as_object().unwrap();
    for kind in [
        "rlib",
        "rmeta",
        "object file",
        "dep-info",
        "fingerprint",
        "build script executable",
        "build script output",
        "binary",
    ] {
        assert!(
            kinds.get(kind).is_some_and(|b| b.as_u64().unwrap() > 0),
            "{kind} missing from {kinds:?}"
        );
    }

    let crates = report["by_crate"].as_object().unwrap();
    assert!(crates.get("dep").is_some_and(|b| b.as_u64().unwrap() > 0));
    assert!(crates.get("app").is_some_and(|b| b.as_u64().unwrap() > 0));

    let text = run_in(root, &["--target-dir", root.to_str().unwrap()]);
    text.assert_ok();
    assert!(
        text.stdout.contains("build script executable"),
        "{}",
        text.stdout
    );

    #[cfg(unix)]
    assert_matches_du(report["total_bytes"].as_u64().unwrap(), root);
}

#[test]
fn a_dsym_and_split_dwarf_count_as_debuginfo() {
    let dir = TempDir::new("debuginfo");
    let root = dir.path();
    let dsym = write_sized(
        root,
        "debug/deps/libfoo-0123456789abcdef.dSYM/Contents/Resources/DWARF/foo",
        8192,
        b'g',
    );
    let dwo = write_sized(root, "debug/deps/foo-0123456789abcdef.dwo", 4096, b'w');
    let dwp = write_sized(root, "debug/deps/foo-0123456789abcdef.dwp", 4096, b'p');

    let report = report(root, &["--target-dir", root.to_str().unwrap()]);
    assert_consistent(&report);
    let kinds = report["by_kind"].as_object().unwrap();
    assert!(kinds["debuginfo (dSYM)"].as_u64().unwrap() > 0);
    assert!(kinds["debuginfo (split DWARF)"].as_u64().unwrap() > 0);

    #[cfg(unix)]
    {
        assert_eq!(kinds["debuginfo (dSYM)"], disk_bytes(&dsym));
        assert_eq!(
            kinds["debuginfo (split DWARF)"],
            disk_bytes(&dwo) + disk_bytes(&dwp)
        );
        assert_eq!(
            report["by_content"]["debuginfo"],
            disk_bytes(&dsym) + disk_bytes(&dwo) + disk_bytes(&dwp)
        );
    }
}

#[test]
fn hard_links_are_counted_once_as_the_artifact() {
    let dir = TempDir::new("hardlinks");
    let root = dir.path();
    let rlib = write_sized(
        root,
        "debug/deps/libfoo-0123456789abcdef.rlib",
        64 * 1024,
        b'r',
    );
    // rustc hard-links the compiled artifacts into incremental/.
    hard_link(
        &rlib,
        &root.join("debug/incremental/foo-2k1x9/dep-graph.bin"),
    );
    // A file that exists only in incremental/ is what deleting it would free.
    let only_incremental = write_sized(
        root,
        "debug/incremental/foo-2k1x9/query-cache.bin",
        8192,
        b'q',
    );

    let report = report(root, &["--target-dir", root.to_str().unwrap()]);
    assert_consistent(&report);
    let kinds = report["by_kind"].as_object().unwrap();

    assert_eq!(
        kinds["rlib"],
        disk_bytes(&rlib),
        "the artifact in deps/ must be counted once"
    );
    assert_eq!(
        kinds["incremental"],
        disk_bytes(&only_incremental),
        "the hard link must not add bytes to incremental"
    );
    assert_eq!(
        report["by_content"]["incremental cache"],
        disk_bytes(&only_incremental)
    );
    assert_eq!(report["duplicated_metadata_bytes"], 0);

    #[cfg(unix)]
    assert_matches_du(report["total_bytes"].as_u64().unwrap(), root);
}

#[test]
fn symlinks_are_counted_but_not_followed() {
    #[cfg(unix)]
    {
        let dir = TempDir::new("symlinks");
        let root = dir.path();
        let external = mkdir(root, "external");
        let big = write_sized(&external, "big.bin", 8 * 1024 * 1024, b'B');
        let target = mkdir(root, "target");
        symlink(
            &big,
            &target.join("debug/deps/libbig-0123456789abcdef.rlib"),
        );
        symlink(
            &root.join("does-not-exist"),
            &target.join("debug/deps/broken-0123456789abcdef.rmeta"),
        );
        symlink(&external, &target.join("debug/deps/dirlink"));

        let report = report(dir.path(), &["--target-dir", target.to_str().unwrap()]);
        assert_consistent(&report);
        let total = report["total_bytes"].as_u64().unwrap();
        assert!(
            total < 1024 * 1024,
            "an 8 MiB symlink target leaked into the total: {total}"
        );
        // Symlinks themselves take no space here; none of the external files
        // may appear in the report, even under a zero-valued key.
        for kind in ["binary", "doc", "package"] {
            if let Some(bytes) = report["by_kind"].get(kind) {
                assert_eq!(
                    bytes.as_u64().unwrap(),
                    0,
                    "symlink target leaked into {kind}: {report}"
                );
            }
        }
    }
}

#[test]
fn a_target_directory_of_files_only_still_adds_up() {
    let dir = TempDir::new("files-only");
    let root = dir.path();
    write_sized(root, "CACHEDIR.TAG", 177, b'x');
    write_sized(root, ".rustc_info.json", 1967, b'x');
    let report = report(root, &["--target-dir", root.to_str().unwrap()]);
    assert_consistent(&report);
    assert_eq!(
        report["by_kind"]["other"].as_u64().unwrap()
            + report["by_kind"]["directories"].as_u64().unwrap(),
        report["total_bytes"].as_u64().unwrap()
    );
    assert!(report["by_kind"]["other"].as_u64().unwrap() > 0);
    #[cfg(unix)]
    assert_matches_du(report["total_bytes"].as_u64().unwrap(), root);
}
