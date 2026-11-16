//! Command-line contract: flags, help, exit codes and JSON shape.
//!
//! These tests do not build anything; they exercise argument handling and
//! measuring directories this file creates.

mod common;

use std::fs;

use common::*;

#[test]
fn version_matches_the_package_version() {
    let expected = format!("cargo-heft {}\n", env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        let out = run(&[flag]);
        out.assert_ok();
        assert_eq!(out.stdout, expected, "{flag}");
        assert!(out.stderr.is_empty(), "{flag}");
    }
}

#[test]
fn help_documents_every_option() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag]);
        out.assert_ok();
        assert!(out.stderr.is_empty(), "{flag}");
        for needle in [
            "Usage: cargo heft",
            "--target-dir <DIR>",
            "--top <N>",
            "--json",
            "--what-if",
            "--profile <NAME>",
            "-V, --version",
        ] {
            assert!(
                out.stdout.contains(needle),
                "{flag} does not document {needle}:\n{}",
                out.stdout
            );
        }
    }
}

#[test]
fn unknown_arguments_fail_with_usage() {
    let out = run(&["--definitely-not-a-flag"]);
    out.assert_failed();
    assert!(out.stdout.is_empty());
    assert!(
        out.stderr
            .contains("unknown argument `--definitely-not-a-flag`"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("Usage: cargo heft"), "{}", out.stderr);
}

#[test]
fn options_that_need_a_value_report_usage() {
    for arg in ["--target-dir", "--top", "--profile"] {
        let out = run(&[arg]);
        out.assert_failed();
        assert!(
            out.stderr.contains("Usage: cargo heft"),
            "{arg} without a value: {}",
            out.stderr
        );
    }
}

#[test]
fn invalid_top_values_are_rejected() {
    for value in ["nope", "-1", "1.5", "18446744073709551616"] {
        let out = run(&["--top", value]);
        out.assert_failed();
        assert!(
            out.stderr.contains("Usage: cargo heft"),
            "--top {value}: {}",
            out.stderr
        );
    }
}

#[test]
fn heft_subcommand_prefix_is_accepted() {
    let dir = TempDir::new("heft-prefix");
    write_sized(
        dir.path(),
        "debug/deps/libfoo-0123456789abcdef.rlib",
        4096,
        b'x',
    );
    let target = dir.path().to_str().expect("UTF-8 path");

    let plain = run_in(dir.path(), &["--json", "--target-dir", target]).json();
    let prefixed = run_in(dir.path(), &["heft", "--json", "--target-dir", target]).json();

    assert_eq!(plain, prefixed);
    assert_consistent(&prefixed);
}

#[test]
fn target_dir_must_exist_and_be_a_directory() {
    let dir = TempDir::new("bad-target");
    let missing = dir.join("never-built");
    let out = run_in(
        dir.path(),
        &["--json", "--target-dir", missing.to_str().unwrap()],
    );
    out.assert_failed();
    assert!(out.stderr.contains("does not exist"), "{}", out.stderr);
    assert!(out.stderr.contains("cargo build"), "{}", out.stderr);
    assert!(!out.stderr.contains("os error"), "{}", out.stderr);
    assert!(out.stdout.is_empty(), "JSON must not be printed on error");

    let file = write_file(dir.path(), "a-file", b"not a target directory");
    let out = run_in(dir.path(), &["--target-dir", file.to_str().unwrap()]);
    out.assert_failed();
    assert!(out.stderr.contains("is not a directory"), "{}", out.stderr);
}

#[test]
fn text_output_has_every_table() {
    let dir = TempDir::new("text-tables");
    write_sized(
        dir.path(),
        "debug/deps/libfoo-0123456789abcdef.rlib",
        8192,
        b'x',
    );
    write_sized(
        dir.path(),
        "debug/deps/libbar-0123456789abcdef.rmeta",
        4096,
        b'x',
    );
    write_file(dir.path(), "debug/deps/app-0123456789abcdef.d", b"x");

    let out = run_in(
        dir.path(),
        &["--target-dir", dir.path().to_str().unwrap(), "--top", "1"],
    );
    out.assert_ok();
    for table in ["by kind", "by content", "by profile", "largest crates"] {
        assert!(
            out.stdout.contains(table),
            "missing table {table}:\n{}",
            out.stdout
        );
    }
    assert!(out.stdout.contains("on disk"), "{}", out.stdout);
    assert_eq!(crate_names(&out.stdout).len(), 1);
    let kinds = section_rows(&out.stdout, "by kind");
    assert!(kinds.iter().any(|row| row.starts_with("rlib")), "{kinds:?}");
    assert!(
        kinds.iter().any(|row| row.starts_with("rmeta")),
        "{kinds:?}"
    );
    for row in kinds {
        assert!(row.contains('%'), "row without a percentage: {row}");
    }
}

#[test]
fn top_controls_only_the_crate_table() {
    let dir = TempDir::new("top");
    // Distinct sizes so the ranking is unambiguous.
    write_sized(
        dir.path(),
        "debug/deps/libaaa-0123456789abcdef.rlib",
        12 * 4096,
        b'a',
    );
    write_sized(
        dir.path(),
        "debug/deps/libbbb-0123456789abcdef.rlib",
        8 * 4096,
        b'b',
    );
    write_sized(
        dir.path(),
        "debug/deps/libccc-0123456789abcdef.rlib",
        4 * 4096,
        b'c',
    );
    let target = dir.path().to_str().unwrap();

    let all = run_in(dir.path(), &["--target-dir", target]);
    all.assert_ok();
    assert_eq!(crate_names(&all.stdout), ["aaa", "bbb", "ccc"]);

    let two = run_in(dir.path(), &["--target-dir", target, "--top", "2"]);
    two.assert_ok();
    assert_eq!(crate_names(&two.stdout), ["aaa", "bbb"]);

    let none = run_in(dir.path(), &["--target-dir", target, "--top", "0"]);
    none.assert_ok();
    assert!(crate_names(&none.stdout).is_empty(), "{}", none.stdout);

    let wide = run_in(dir.path(), &["--target-dir", target, "--top", "99"]);
    assert_eq!(crate_names(&wide.stdout), ["aaa", "bbb", "ccc"]);

    // --top is a display option; the machine-readable report ignores it.
    let report = report(dir.path(), &["--target-dir", target, "--top", "1"]);
    assert_eq!(report["by_crate"].as_object().unwrap().len(), 3);
}

#[test]
fn relative_target_dirs_are_reported_as_given() {
    let dir = TempDir::new("relative");
    write_sized(
        dir.path(),
        "target/debug/deps/libfoo-0123456789abcdef.rlib",
        4096,
        b'x',
    );
    let report = report(dir.path(), &["--target-dir", "target"]);
    assert_eq!(report["target_dir"], "target");
    assert_consistent(&report);
}

#[test]
fn cargo_target_dir_environment_is_honoured() {
    let dir = TempDir::outside("cargo-target-env");
    let target = dir.join("build-directory");
    write_sized(
        &target,
        "debug/deps/libfoo-0123456789abcdef.rlib",
        4096,
        b'x',
    );
    let out = run_with(
        dir.path(),
        &["--json"],
        &[("CARGO_TARGET_DIR", target.to_str().unwrap())],
        &[],
    );
    let report = out.json();
    assert_eq!(report["target_dir"], target.to_str().unwrap());
    assert_consistent(&report);
    assert!(report["total_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn outside_a_cargo_workspace_the_error_is_cargo_s_own() {
    let dir = TempDir::outside("not-a-workspace");
    let out = run_in(dir.path(), &["--json"]);
    out.assert_failed();
    assert!(
        out.stderr.contains("Cargo.toml"),
        "expected cargo's error, got:\n{}",
        out.stderr
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn paths_with_spaces_and_unicode_round_trip() {
    let dir = TempDir::new("unicode");
    let target = dir.join("alvo com espaço-ção-🦀");
    write_sized(
        &target,
        "debug/deps/libfoo-0123456789abcdef.rlib",
        4096,
        b'x',
    );
    let report = report(dir.path(), &["--target-dir", target.to_str().unwrap()]);
    assert_eq!(report["target_dir"], target.to_str().unwrap());
    assert_consistent(&report);
    assert!(report["total_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn a_target_dir_symlink_to_a_directory_is_followed() {
    #[cfg(unix)]
    {
        let dir = TempDir::new("target-symlink");
        let real = mkdir(dir.path(), "real-target");
        write_sized(&real, "debug/deps/libfoo-0123456789abcdef.rlib", 4096, b'x');
        let link = dir.join("link");
        symlink(&real, &link);
        let report = report(dir.path(), &["--target-dir", link.to_str().unwrap()]);
        assert_consistent(&report);
        // Measured where it points, so the total is that of `du -s real`
        // (`du -sL link`), not the few bytes of the link itself.
        assert_matches_du(report["total_bytes"].as_u64().unwrap(), &real);
        let out = run_in(dir.path(), &["--target-dir", link.to_str().unwrap()]);
        out.assert_ok();
        let real = real.canonicalize().unwrap();
        assert!(
            out.stdout
                .starts_with(&format!("{} -> {}:", link.display(), real.display())),
            "{}",
            out.stdout
        );
    }
}

#[test]
fn json_is_absent_from_stdout_when_measurement_fails() {
    let dir = TempDir::new("json-error");
    let missing = dir.join("missing");
    assert!(missing.to_str().is_some());
    let out = run_in(
        dir.path(),
        &["--json", "--target-dir", missing.to_str().unwrap()],
    );
    out.assert_failed();
    assert!(out.stdout.is_empty());
    assert!(out.stderr.contains("nothing to measure"), "{}", out.stderr);
}

#[test]
fn duplicate_target_dir_flags_use_the_last_one() {
    let dir = TempDir::new("duplicate-flags");
    let first = mkdir(dir.path(), "first");
    let second = mkdir(dir.path(), "second");
    write_sized(
        &first,
        "debug/deps/libone-0123456789abcdef.rlib",
        4096,
        b'1',
    );
    write_sized(
        &second,
        "debug/deps/libtwo-0123456789abcdef.rlib",
        8192,
        b'2',
    );

    let out = run_in(
        dir.path(),
        &[
            "--json",
            "--target-dir",
            first.to_str().unwrap(),
            "--target-dir",
            second.to_str().unwrap(),
        ],
    );
    let report = out.json();
    assert_eq!(report["target_dir"], second.to_str().unwrap());
    assert!(report["by_crate"].get("two").is_some());
    assert!(report["by_crate"].get("one").is_none());
}

#[test]
fn an_empty_directory_is_a_valid_target() {
    let dir = TempDir::new("empty");
    let out = run_in(
        dir.path(),
        &["--json", "--target-dir", dir.path().to_str().unwrap()],
    );
    let report = out.json();
    assert_consistent(&report);
    assert_eq!(
        report["by_kind"]["directories"], report["total_bytes"],
        "an empty directory contains only its own directory entry"
    );
    assert_eq!(report["duplicated_metadata_bytes"], 0);

    let text = run_in(dir.path(), &["--target-dir", dir.path().to_str().unwrap()]);
    text.assert_ok();
    assert!(text.stdout.contains("on disk"), "{}", text.stdout);
}

#[test]
fn unreadable_directories_are_reported_not_panicked() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("unreadable");
        let target = mkdir(dir.path(), "target");
        let secret = mkdir(&target, "debug/secret");
        write_file(&secret, "blob", b"x");
        let _ = fs::set_permissions(&secret, fs::Permissions::from_mode(0o000));

        let out = run_in(dir.path(), &["--target-dir", target.to_str().unwrap()]);
        // Restore permissions before any assertion can unwind, so the
        // temporary directory can still be removed.
        let _ = fs::set_permissions(&secret, fs::Permissions::from_mode(0o755));

        // Under root, permissions are not enforced; accept either outcome but
        // never a panic.
        if out.status.success() {
            return;
        }
        out.assert_failed();
        // The error names the directory, not just the OS error, and says
        // why nothing was reported.
        assert!(
            out.stderr
                .contains(&format!("cannot read {}", secret.display())),
            "{}",
            out.stderr
        );
        assert!(
            out.stderr.contains("Nothing was measured"),
            "{}",
            out.stderr
        );
        assert!(out.stdout.is_empty(), "{}", out.stdout);
    }
}

#[test]
fn flags_that_would_be_ignored_are_rejected() {
    let dir = TempDir::new("misplaced");
    let target = dir.path().to_str().unwrap();
    for (args, expect) in [
        (
            &["--target-dir", target, "--profile", "release"][..],
            "--profile only applies to --what-if",
        ),
        (
            &["--target-dir", target, "--", "--release"][..],
            "only apply to --what-if",
        ),
        (
            &["--what-if", "--target-dir", target][..],
            "--target-dir does not apply to --what-if",
        ),
        (
            &["--what-if", "--top", "3"][..],
            "--top does not apply to --what-if",
        ),
    ] {
        let out = run(args);
        out.assert_failed();
        assert!(out.stderr.contains(expect), "{args:?}: {}", out.stderr);
        assert!(
            out.stdout.is_empty(),
            "{args:?} printed a report: {}",
            out.stdout
        );
    }
}

#[test]
fn a_value_can_follow_an_equals_sign() {
    let dir = TempDir::new("equals");
    write_sized(
        dir.path(),
        "debug/deps/libfoo-0123456789abcdef.rlib",
        4096,
        b'x',
    );
    let spaced = report(dir.path(), &["--target-dir", "."]);
    let joined = report(dir.path(), &["--target-dir=."]);
    assert_eq!(spaced, joined);
    run_in(dir.path(), &["--target-dir=.", "--top=1"]).assert_ok();

    let out = run_in(dir.path(), &["--json=yes", "--target-dir=."]);
    out.assert_failed();
    assert!(
        out.stderr.contains("--json takes no value"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_bad_top_value_is_named_in_the_error() {
    let out = run(&["--top", "many"]);
    out.assert_failed();
    assert!(
        out.stderr
            .contains("--top needs a whole number, got `many`"),
        "{}",
        out.stderr
    );
    let out = run(&["--top"]);
    out.assert_failed();
    assert!(out.stderr.contains("--top needs a value"), "{}", out.stderr);
}

#[test]
fn the_header_gives_the_total_in_du_h_units_too() {
    let dir = TempDir::new("mib");
    write_sized(
        dir.path(),
        "debug/deps/libfoo-0123456789abcdef.rlib",
        2 * 1024 * 1024,
        b'x',
    );
    let out = run_in(dir.path(), &["--target-dir", "."]);
    out.assert_ok();
    let header = out.stdout.lines().next().unwrap();
    assert!(header.contains(" on disk ("), "{header}");
    assert!(header.contains(" MiB in the units du -h uses)"), "{header}");
    let help = run(&["--help"]);
    assert!(help.stdout.contains("Examples:"), "{}", help.stdout);
    let usage = run(&["--top"]);
    assert!(
        !usage.stderr.contains("Examples:"),
        "errors keep the short usage"
    );
}
