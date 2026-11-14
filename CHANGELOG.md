# Changelog

Notable changes to cargo-heft. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `--version`.
- A declared minimum supported Rust version (1.88), tested in CI.

### Fixed

- An argument that is not valid UTF-8, such as a path, is handled as a path
  instead of ending in a panic.
- `cargo heft | head` and other readers that close the pipe early no longer
  cause a panic.
- Sizes pick their unit after rounding and show plain bytes below 1000:
  `0 B` and `1.0 MB` where it used to say `0.0 kB` and `1000.0 kB`.

### Changed

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

[Unreleased]: https://github.com/eltonlaice/cargo-heft/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/eltonlaice/cargo-heft/releases/tag/v0.1.0
