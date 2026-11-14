# Target directory sizes of 17 Rust projects, Rust 1.99.1

The same projects measured with Rust 1.100 are in
[`../2026-11-14-rust-1.100`](../2026-11-14-rust-1.100).

Each project was cloned at the commit shown, built from scratch with
`cargo +stable build` (the `dev` profile, as the project configures it) and
measured with `cargo heft --json`. One JSON file per project is in this
directory, with a `.meta` file holding the commit and toolchain.

- Toolchain: rustc 1.99.1, host `aarch64-apple-darwin` (macOS, Apple silicon).
- Reproduce: `bench/run.sh <work-dir> <out-dir>`, then `python3 bench/table.py <out-dir>`.

| Project | Commit | `target/` | Debuginfo | Incremental | Metadata | of which twice | Code |
|---|---|--:|--:|--:|--:|--:|--:|
| clap | `ec93e71` | 82 MB | 29% | 42% | 7% | 4% | 21% |
| fd | `6cf3302` | 189 MB | 2% | 9% | 46% | 23% | 43% |
| axum | `3ebdbe6` | 272 MB | 19% | 26% | 35% | 16% | 18% |
| tokio | `f3e855f` | 273 MB | 27% | 32% | 24% | 10% | 17% |
| hyperfine | `c516152` | 275 MB | 18% | 8% | 45% | 22% | 29% |
| ripgrep | `178cc02` | 314 MB | 29% | 22% | 19% | 9% | 30% |
| alacritty | `aa5d942` | 729 MB | 25% | 15% | 30% | 15% | 30% |
| bat | `8cf33bd` | 1.02 GB | 31% | 4% | 28% | 14% | 36% |
| tantivy | `2ad36b4` | 1.26 GB | 35% | 21% | 19% | 9% | 25% |
| zola | `48d4e7a` | 1.98 GB | 0% | 9% | 46% | 23% | 42% |
| starship | `d128ba8` | 2.35 GB | 36% | 8% | 24% | 12% | 31% |
| rust-analyzer | `114dcb8` | 4.43 GB | 22% | 29% | 10% | 5% | 38% |
| typst | `84cca40` | 5.26 GB | 54% | 12% | 17% | 8% | 17% |
| uv | `d5c3f17` | 5.97 GB | 20% | 27% | 13% | 6% | 39% |
| ruff | `b140af2` | 6.35 GB | 22% | 32% | 14% | 6% | 31% |
| nushell | `3e2dc65` | 6.63 GB | 36% | 17% | 17% | 8% | 29% |
| wasmtime | `c52bc6d` | 7.56 GB | 36% | 27% | 11% | 5% | 25% |
| **all 17** | | **44.9 GB** | **30%** | **22%** | **17%** | **8%** | **30%** |

Not measured: cargo, whose current source needs rustc 1.100.

## Reading the table

- Percentages are shares of the project's `target/` on disk. Debuginfo,
  incremental, metadata and code add up to nearly 100%; the rest is small
  files such as fingerprints and dep-info.
- "Metadata" is crate metadata in `.rmeta` files plus the copy inside each
  `.rlib`. "Of which twice" is the part that exists in both places.
- "Incremental" is what deleting `incremental/` would free. Files that rustc
  hard-links between `deps/` and `incremental/` count as the `deps/` artifact.

## What stands out

- Over the 17 projects, 30% of the bytes are debuginfo, 22% the incremental
  cache, 17% metadata, and 8% is metadata stored a second time.
- Projects that turn debuginfo down in their own `dev` profile show it:
  zola sets `debug = 0` (0% debuginfo) and fd sets `line-tables-only` with no
  debuginfo for dependencies (2%). In both, metadata becomes the largest part,
  at 46%.
- typst is the other extreme: 54% of 5.26 GB is debuginfo.

## Limits

- One platform. On macOS debuginfo stays in loose object files next to the
  `.rlib`; on Linux it is inside the `.rlib`, so the split by file kind
  differs there. The split by content should be comparable, but it has not
  been measured.
- One build per project, default members of each workspace, no tests.
