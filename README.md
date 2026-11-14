# cargo-heft

[![CI](https://github.com/eltonlaice/cargo-heft/actions/workflows/ci.yml/badge.svg)](https://github.com/eltonlaice/cargo-heft/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cargo-heft.svg)](https://crates.io/crates/cargo-heft)

Find out what takes up space in your Cargo `target/` directory.

```
cargo install cargo-heft
cargo heft
```

It prints the size of the directory four ways: by kind of file, by what is
inside the files, by profile and by crate. This is ripgrep after a fresh
`dev` build:

```
target: 307.2 MB on disk

by kind
  rlib                              90.6 MB   29.5%
  object file                       79.7 MB   25.9%
  incremental                       67.9 MB   22.1%
  binary                            31.7 MB   10.3%
  rmeta                             30.2 MB    9.8%
  build script executable            5.8 MB    1.9%
  fingerprint                      983.0 kB    0.3%
  dep-info                         282.6 kB    0.1%
  build script output              131.1 kB    0.0%
  other                              8.2 kB    0.0%
  directories                           0 B    0.0%

by content
  debuginfo                         89.9 MB   29.3%
  code, data and symbols            87.7 MB   28.5%
  incremental cache                 67.9 MB   22.1%
  metadata (.rmeta files)           30.2 MB    9.8%
  metadata (inside rlib/dylib)      30.1 MB    9.8%
  other                              1.4 MB    0.5%
  note: 30.1 MB of metadata is stored twice, as a .rmeta file and inside its .rlib

by profile
  debug                            307.2 MB  100.0%
  (not in a profile)                 8.2 kB    0.0%

largest crates
  ignore                            39.6 MB   12.9%
  regex_automata                    38.1 MB   12.4%
  ripgrep                           37.4 MB   12.2%
  rg                                25.8 MB    8.4%
  regex_syntax                      22.9 MB    7.5%
  globset                           14.4 MB    4.7%
```

## What would a setting save?

`cargo heft --what-if` builds the workspace from scratch once per profile
setting, in a temporary directory, and compares the sizes. Nothing in your
own `target/` is touched.

```
fresh `dev` builds, settings as they would go in [profile.dev]

  setting                                  size  vs first    build
  as configured                        308.1 MB               8.5s
  debug = "line-tables-only"           245.5 MB    -20.3%     6.7s
  debug = false                        187.0 MB    -39.3%     6.3s
  incremental = false                  216.8 MB    -29.6%     5.9s
  line-tables-only, no incremental     172.1 MB    -44.1%     5.4s
```

It runs five full builds, so it takes five times a clean build.
`incremental = false` makes rebuilds after an edit slower: it suits CI more
than local work.

## On real projects

Eighteen well-known projects, 47.5 GB of `target/` in total, `dev` profile:

| Debuginfo | Incremental cache | Metadata | of which stored twice | Code |
|--:|--:|--:|--:|--:|
| 31% | 21% | 17% | 8% | 30% |

Per-project tables, the raw JSON and the scripts to repeat the measurements
are in [`bench/results`](bench/results): the
[18 projects](bench/results/2026-11-14-rust-1.100) and
[`--what-if` on five of them](bench/results/2026-11-14-what-if).

## How it counts

- Sizes are on-disk: allocated blocks, not file lengths. The total matches
  `du -s`. Units are decimal (1 MB = 1 000 000 bytes), so the figure reads
  lower than `du -h`, which counts in units of 1024.
- Every inode is counted once. rustc hard-links object files and metadata
  between the compiled artifacts and `incremental/`; a shared file counts as
  the artifact, so "incremental" is what deleting that directory would free.
- "By content" opens `.rlib` archives, object files and binaries and splits
  each file's on-disk bytes into metadata, debuginfo and the rest, in
  proportion to the bytes found inside.
- Files are classified by name. Both target layouts are understood: the
  classic one and the build-dir layout that Cargo uses from 1.100.

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

Values can be written `--flag value` or `--flag=value`. A flag that does not
apply to the mode you asked for is an error, not something silently ignored.

## Limits

- macOS and Linux only. On Windows the code falls back to file lengths and
  cannot detect hard links, so the numbers would not be reliable.
- The content view reads ELF, Mach-O and COFF. Other formats, such as
  WebAssembly and LLVM bitcode, count entirely as code, and `.a` static
  libraries are not opened.
- Crates are told apart by file and directory names, so the per-crate table
  is an attribution, not an exact account. The total and the other three
  views cover every byte.
- It measures; it does not delete anything. For cleaning there are
  `cargo clean`, [cargo-sweep](https://crates.io/crates/cargo-sweep) and
  [kondo](https://crates.io/crates/kondo).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in
[CHANGELOG.md](CHANGELOG.md).

## License

MIT or Apache-2.0, at your option.
