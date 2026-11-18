# cargo-heft

[![CI](https://github.com/eltonlaice/cargo-heft/actions/workflows/ci.yml/badge.svg)](https://github.com/eltonlaice/cargo-heft/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cargo-heft.svg)](https://crates.io/crates/cargo-heft)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-blue.svg)](https://github.com/eltonlaice/cargo-heft)

cargo-heft tells you what takes up space in a Cargo `target/` directory, and
how much each profile setting would save. `cargo heft disk` does the same for
what Rust keeps in your Cargo home and rustup home.

A `target/` directory easily grows to several gigabytes. Disk usage is the
second most common complaint about the Rust toolchain in the
[2025 State of Rust survey], and the Rust project has a [goal] to reduce it.
`du` gives you one number. cargo-heft splits it by kind of file, by crate,
and by what is inside the files: debuginfo, metadata and code.

It only reads: nothing in your `target/`, Cargo home or rustup home is changed
or deleted. It runs on
macOS and Linux.

[2025 State of Rust survey]: https://blog.rust-lang.org/2026/03/02/2025-State-Of-Rust-Survey-results/
[goal]: https://goals.rust-lang.org/2026/target-dir-size.html

## [Change log](CHANGELOG.md)

## Features

- Sizes are what the files take on disk, and the total matches `du -s`.
- Hard links are counted once, so the numbers add up.
- Four views: by kind of file, by content, by profile and by crate.
- Looks inside `.rlib` files, objects and binaries to separate metadata,
  debuginfo and code.
- Points out metadata that is stored twice, as a `.rmeta` file and inside
  the `.rlib`.
- `--what-if` builds your workspace once per profile setting and shows how
  much each one would save.
- Understands both the classic target layout and the build-dir layout of
  Cargo 1.100.
- JSON output for scripts and CI (not yet for `cargo heft disk`).
- `cargo heft disk` measures the Cargo home and the rustup home by part and
  says what removing each part would cost.

See [how it counts](docs/how-it-counts.md) for the details and the limits.

## Usage

```
cargo install cargo-heft
```

Then, in a workspace you have built:

```
cargo heft              # measure the target directory
cargo heft --top 5      # list only the five largest crates
cargo heft --what-if    # what would each profile setting save?
cargo heft --json       # machine-readable output
cargo heft disk         # what Rust keeps in ~/.cargo and ~/.rustup
```

Part of the output for ripgrep after a `dev` build:

```
target: 307.7 MB on disk (293.4 MiB in the units du -h uses)

by content
  debuginfo                         89.9 MB   29.2%
  code, data and symbols            87.7 MB   28.5%
  incremental cache                 68.3 MB   22.2%
  metadata (.rmeta files)           30.2 MB    9.8%
  metadata (inside rlib/dylib)      30.1 MB    9.8%
  other                              1.4 MB    0.5%
  note: 30.1 MB of metadata is stored twice, as a .rmeta file and inside its .rlib
```

And `--what-if` on the same project:

```
  setting                                  size  vs first    build
  as configured                        308.1 MB               8.5s
  debug = "line-tables-only"           245.5 MB    -20.3%     6.7s
  debug = false                        187.0 MB    -39.3%     6.3s
  incremental = false                  216.8 MB    -29.6%     5.9s
  line-tables-only, no incremental     172.1 MB    -44.1%     5.4s
```

And `cargo heft disk` on a development machine (shortened):

```
Cargo home: ~/.cargo (4.83 GB, 4609.2 MiB in the units du -h uses)
  registry/src       3.27 GB  rebuilt   cargo +nightly -Zgc clean gc
  git/checkouts     923.7 MB  rebuilt   cargo +nightly -Zgc clean gc
  registry/cache    477.5 MB  download  cargo +nightly -Zgc clean gc
  bin                18.4 MB  keep

rustup home: ~/.rustup (4.25 GB, 4057.1 MiB in the units du -h uses)
  toolchains/stable-aarch64-apple-darwin      1.68 GB  keep      (in use: default)
  toolchains/1.99.1-aarch64-apple-darwin      1.43 GB  download  rustup toolchain uninstall 1.99.1-aarch64-apple-darwin
```

It removes nothing. What each part is and what removing it costs is in
[docs/disk.md](docs/disk.md).

## Options

- `--target-dir <DIR>`: measure this directory instead of the workspace's.
- `--top <N>`: how many crates to list (default 15).
- `--json`: machine-readable output. It has a `schema_version`, which goes up
  when a key is removed or renamed or a value changes meaning.
- `--what-if`: compare profile settings.
- `--profile <NAME>`: the profile `--what-if` builds (default `dev`).
- `--only <IDS>`: the settings `--what-if` builds, comma-separated, such as
  `--only configured,line-tables`. The ids are `configured`, `line-tables`,
  `no-debug`, `no-incremental` and `line-tables-no-incremental`.
- `-- <args>`: extra arguments for `cargo build` in `--what-if`, such as
  `-p my-crate`.
- `disk`: measure the Cargo home (`CARGO_HOME`, default `~/.cargo`) and the
  rustup home (`RUSTUP_HOME`, default `~/.rustup`) instead of a target
  directory. It takes no options yet.

## Results

On 18 well-known projects (ripgrep, tokio, cargo, rust-analyzer, ruff, uv and
others), a quarter to a third of `target/` is debuginfo, and a fifth to a
third is the incremental cache. A recent nightly needs 19% less space than
Rust 1.100, about half of that because metadata is no longer stored twice. The tables,
the raw data and the scripts are in [`bench/results`](bench/results).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of:

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   https://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   https://opensource.org/license/mit)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
