# Target directory sizes of 18 Rust projects, on Linux

The same 18 projects, at the same commits, as the macOS run in
[`../2026-11-14-rust-1.100`](../2026-11-14-rust-1.100), built from scratch
with `cargo build` (the `dev` profile, as each project configures it) and
measured with `cargo heft --json`.

- Toolchain: rustc 1.100.0, host `x86_64-unknown-linux-gnu`.
- Measured on 15 Nov 2026 on GitHub Actions `ubuntu-latest` runners, one job
  per project, by the workflow in `.github/workflows/bench.yml`.
- `CARGO_INCREMENTAL=1` is set: Cargo 1.100 turns incremental off when `CI`
  is set, and a developer's build has it on.
- cargo-heft was built from the commit before the fix that lists a target
  under its package, so `by_crate` in these JSON files can have a crate
  under two names. The table does not use `by_crate`.

| Project | Commit | `target/` | Debuginfo | Incremental | Metadata | of which twice | Code | Other |
|---|---|--:|--:|--:|--:|--:|--:|--:|
| clap | `ec93e71` | 101 MB | 21% | 55% | 6% | 3% | 17% | 1% |
| tokio | `f3e855f` | 289 MB | 18% | 46% | 20% | 10% | 16% | 1% |
| axum | `3ebdbe6` | 293 MB | 19% | 33% | 32% | 16% | 16% | 1% |
| hyperfine | `c516152` | 380 MB | 33% | 11% | 37% | 19% | 17% | 1% |
| ripgrep | `178cc02` | 384 MB | 29% | 33% | 17% | 8% | 20% | 1% |
| fd | `6cf3302` | 388 MB | 10% | 6% | 22% | 11% | 29% | 32% |
| bat | `8cf33bd` | 1.14 GB | 35% | 7% | 26% | 13% | 31% | 1% |
| alacritty | `aa5d942` | 1.23 GB | 33% | 17% | 24% | 12% | 26% | 1% |
| tantivy | `2ad36b4` | 1.37 GB | 22% | 39% | 17% | 9% | 21% | 2% |
| zola | `48d4e7a` | 2.06 GB | 0% | 10% | 46% | 23% | 39% | 5% |
| starship | `d128ba8` | 2.42 GB | 33% | 19% | 21% | 10% | 26% | 1% |
| cargo | `382ecb8` | 3.19 GB | 29% | 30% | 13% | 6% | 25% | 3% |
| rust-analyzer | `114dcb8` | 4.23 GB | 21% | 48% | 10% | 5% | 22% | 0% |
| uv | `d5c3f17` | 5.61 GB | 14% | 39% | 14% | 7% | 25% | 8% |
| ruff | `b140af2` | 6.79 GB | 26% | 39% | 11% | 6% | 19% | 6% |
| typst | `84cca40` | 6.93 GB | 35% | 28% | 13% | 7% | 24% | 0% |
| nushell | `3e2dc65` | 8.33 GB | 29% | 30% | 14% | 7% | 26% | 1% |
| wasmtime | `c52bc6d` | 8.58 GB | 24% | 44% | 10% | 5% | 21% | 1% |
| **all 18** | | **53.7 GB** | **25%** | **34%** | **15%** | **7%** | **24%** | **3%** |

"Other" is what the four content columns leave: build script output,
fingerprints, dep-info.

## Against macOS

| | macOS arm64 | Linux x86_64 |
|---|--:|--:|
| Total, 18 projects | 47.5 GB | 53.7 GB |
| Debuginfo | 31% | 25% |
| Incremental | 21% | 34% |
| Metadata | 17% | 15% |
| Code | 30% | 24% |
| Other | 1% | 3% |

By kind of file the two look different, because debuginfo lives in different
places:

| Kind | macOS | Linux |
|---|--:|--:|
| `.rlib` | 38% | 35% |
| incremental | 21% | 34% |
| loose object files | 20% | 0% |
| binaries | 9% | 12% |
| `.rmeta` | 8% | 7% |
| build script executables | 1% | 5% |

What explains the differences, as far as the files show:

- **Incremental.** On macOS rustc leaves the object files next to the
  `.rlib`, because the debuginfo stays in them, and they are hard links to
  the copies in `incremental/`. Each inode is counted once, as the artifact,
  so those bytes are "loose object files" there. On Linux there are no such
  files, and the same objects count as incremental.
- **Build script executables and binaries.** On Linux the debuginfo is
  linked into the executable. typst's build scripts take 226 MB on Linux and
  26 MB on macOS; its binaries, 996 MB and 244 MB.
- **Different code is built.** fd is twice as large on Linux (388 MB against
  183 MB) because it uses jemalloc there and not on macOS: the build script
  of `tikv-jemalloc-sys` leaves 121 MB of output.

## Limits

- One build per project, default members of each workspace, no tests.
- The two platforms differ in architecture as well as in operating system,
  so this is not a comparison of the same machine code.
