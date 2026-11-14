# cargo-heft

[![CI](https://github.com/eltonlaice/cargo-heft/actions/workflows/ci.yml/badge.svg)](https://github.com/eltonlaice/cargo-heft/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cargo-heft.svg)](https://crates.io/crates/cargo-heft)

Find out what takes up space in your Cargo `target/` directory.

```
cargo install cargo-heft
cargo heft
```

```
target: 313.8 MB on disk

by kind
  rlib                              90.6 MB   28.9%
  object file                       80.2 MB   25.5%
  incremental                       68.8 MB   21.9%
  binary                            31.8 MB   10.1%
  rmeta                             29.8 MB    9.5%
  build script executable           11.4 MB    3.6%
  ...

by content
  code, data and symbols            93.6 MB   29.8%
  debuginfo                         90.7 MB   28.9%
  incremental cache                 68.8 MB   21.9%
  metadata (.rmeta files)           29.8 MB    9.5%
  metadata (inside rlib/dylib)      29.8 MB    9.5%
  other                              1.2 MB    0.4%
  note: 29.8 MB of metadata is stored twice, as a .rmeta file and inside its .rlib
```

(ripgrep, fresh `dev` build, macOS arm64, cargo 1.99.1.)

## What the numbers mean

- Sizes are on-disk: allocated blocks, not file lengths. The total matches `du -s`.
  Units are decimal (1 MB = 1 000 000 bytes), so the figure reads lower than
  `du -h`, which counts in units of 1024.
- Every inode is counted once. rustc hard-links object files and metadata
  between `deps/` and `incremental/`; a shared file is counted as the artifact
  in `deps/`, so `incremental` is what deleting that directory would free.
- Files are classified by name, not by directory.
- "By content" opens `.rlib` archives, object files and binaries and splits
  each file's on-disk bytes into metadata, debuginfo and the rest, in
  proportion to the bytes found inside.

## What would a setting save?

`cargo heft --what-if` builds the workspace from scratch once per profile
setting, in a temporary directory, and compares the sizes. Nothing in your
own `target/` is touched.

```
fresh `dev` builds, settings as they would go in [profile.dev]

  setting                                  size  vs first    build
  as configured                        314.0 MB               7.7s
  debug = "line-tables-only"           251.3 MB    -20.0%     6.8s
  debug = false                        191.6 MB    -39.0%     6.0s
  incremental = false                  222.3 MB    -29.2%     6.2s
  line-tables-only, no incremental     177.2 MB    -43.6%     5.8s
```

(ripgrep again.) It runs five full builds, so it takes five times a clean
build. `incremental = false` makes rebuilds after an edit slower: it suits CI
more than local work.

## Options

- `--target-dir <DIR>`: measure this directory instead of the workspace's.
- `--top <N>`: how many crates to list (default 15).
- `--json`: machine-readable output, for both modes. It carries a
  `schema_version`, which goes up when a key is removed or renamed or a value
  changes meaning; new keys can appear without a new version.
- `--what-if`: compare profile settings, as above.
- `--profile <NAME>`: profile for `--what-if` (default `dev`).
- `-- <args>`: extra arguments for `cargo build` in `--what-if`, such as
  `-p my-crate` or `--features x`.

## Status

Early. Works on macOS and Linux, with both the classic target layout and the
build-dir layout that Cargo 1.100 uses. Tested in CI on both systems.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in
[CHANGELOG.md](CHANGELOG.md).

## License

MIT or Apache-2.0, at your option.
