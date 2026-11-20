# Changelog

Notable changes to cargo-heft. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `cargo heft disk --json`, with its own `schema_version`, documented in
  `docs/disk.md`. (#51)

### Changed

- The content view reads only the headers and section tables it needs from
  each `.rlib`, object file and binary, instead of the whole file. Same
  numbers; on a 347 MB target directory peak memory went from 36 MB to
  8 MB. (#9)

## [0.3.1] - 2026-11-19

### Changed

- `cargo heft disk` keeps a toolchain that is in use here and says why: the
  default, a `rustup override`, `RUSTUP_TOOLCHAIN`, or a `rust-toolchain.toml`
  in the current directory or above it. Before, only the default was kept and
  every other toolchain was offered for uninstall. A channel name such as `nightly` keeps
  that toolchain only, not the dated ones (`nightly-2026-11-01`).
- `cargo heft disk` shows how far it has got while it measures a large home,
  on stderr and only when stderr is a terminal.
- `cargo heft disk` also gives each home's size in MiB, the units `du -h`
  uses.

## [0.3.0] - 2026-11-18

### Fixed

- A target directory that is a symlink is measured where it points, so the
  total matches `du -s` on the real path. The first line shows both paths.
  (#27)
- A directory that cannot be read is named in the error, which says that
  nothing was measured, instead of the bare OS error. (#24)

### Added

- `cargo heft disk` measures the Cargo home and the rustup home by part
  (registry index, cache and sources, git clones and checkouts, installed
  tools, each toolchain), says what removing each part would cost, and names
  the command that would do it. It reads only. Design in `docs/disk.md`.
  (#46, #47, #48, #50, #52)
- `--what-if --only <IDS>` builds only the chosen settings, such as
  `--only configured,line-tables`. The JSON output gives each scenario's
  `id`. (#25)
- Ctrl-C during `--what-if` removes the temporary build directory and exits
  with code 130. (#25)
- `.a` static libraries are opened like `.rlib` archives: their debuginfo is
  counted as debuginfo, and they are a kind of their own instead of "other".
  (#26)

## [0.2.2] - 2026-11-16

### Fixed

- `--target-dir` pointed inside a target directory, such as `target/debug`
  or `target/doc`, reads paths as if measured from the root. The profile had
  an empty name and documentation showed up as "other".
- rustdoc's source pages for files a build script generated are
  documentation, not build script output.
- A build script's own `.d` file is dep-info. It was counted with the
  executable.
- When a profile directory does not exist yet, the hint names the command
  that builds it, such as `cargo build --release`.

### Changed

- The first line also gives the total in MiB, the unit `du -h` uses, so the
  two can be compared at a glance.
- Under "by content", a line names the kinds that make up "other".
- `--help` ends with examples. Error messages keep the short usage.

## [0.2.1] - 2026-11-15

### Fixed

- A target whose name differs from its package, such as the `rg` binary of
  `ripgrep`, is listed under the package. In the build-dir v2 layout it
  appeared twice in "largest crates", once under each name.

## [0.2.0] - 2026-11-14

Mostly fixes from a review of 0.1.0. It is 0.2.0 and not 0.1.1 because two
things can break a script that worked: flags that were silently ignored are
now errors, and one key of the JSON output was renamed.

### Added

- `--version`.
- `schema_version` in the `--json` output of both modes, starting at 1. It
  goes up when a key is removed or renamed or a value changes meaning.
- A declared minimum supported Rust version (1.88), tested in CI.
- `--flag=value` is accepted as well as `--flag value`.

### Fixed

- An argument that is not valid UTF-8, such as a path, is handled as a path
  instead of ending in a panic.
- `cargo heft | head` and other readers that close the pipe early no longer
  cause a panic.
- Sizes pick their unit after rounding and show plain bytes below 1000:
  `0 B` and `1.0 MB` where it used to say `0.0 kB` and `1000.0 kB`.
- The files a build script run leaves behind (`output`, `stderr`,
  `root-output`, `invoked.timestamp`) count as build script output in the
  classic layout too, as they already did in the build-dir v2 layout. They
  were under "other".
- "By profile" only lists directories that have the shape of a profile.
  `doc/`, `package/` and files at the top are grouped as
  "(not in a profile)", which replaces "(top level)". A build for another
  target shows as `<triple>/debug` instead of the triple alone.
- The incremental cache of build scripts is listed as "(build scripts)"
  instead of a crate called `build_script_build`.
- Symbolic links inside the target directory are listed as "symlink". They
  were classified by name and mode, usually as "binary", and a link named
  like an artifact was opened and read through.

### Changed

- `--help` starts with a one-line description of the tool.
- Flags that would have been ignored are now errors: `--profile` and
  arguments after `--` without `--what-if`, and `--target-dir` and `--top`
  with it.
- A bad or missing value for `--top` is named in the error.
- A target directory that does not exist is reported as nothing to measure,
  with a hint to build first, instead of a raw OS error.

## [0.1.0] - 2026-11-14

First release.

### Added

- `cargo heft`: size of a target directory by artifact kind, by profile and
  by crate. Sizes are on-disk, every inode is counted once, and the total
  matches `du`.
- A view by content: metadata, debuginfo and code inside `.rlib` files,
  object files and binaries, with a note when metadata is stored twice.
- `--what-if`: builds the workspace once per profile setting in a temporary
  directory and compares the sizes.
- `--json` for both modes.
- Support for the classic target layout and the build-dir v2 layout of
  Cargo 1.100.

[Unreleased]: https://github.com/eltonlaice/cargo-heft/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/eltonlaice/cargo-heft/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/eltonlaice/cargo-heft/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/eltonlaice/cargo-heft/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/eltonlaice/cargo-heft/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/eltonlaice/cargo-heft/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/eltonlaice/cargo-heft/releases/tag/v0.1.0
