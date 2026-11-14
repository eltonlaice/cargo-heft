# Changelog

Notable changes to cargo-heft. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/eltonlaice/cargo-heft/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/eltonlaice/cargo-heft/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/eltonlaice/cargo-heft/releases/tag/v0.1.0
