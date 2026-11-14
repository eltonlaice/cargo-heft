# Target directory sizes of 18 Rust projects, Rust 1.100

Each project was cloned at the commit shown, built from scratch with
`cargo build` (the `dev` profile, as the project configures it) and measured
with `cargo heft --json`. One JSON file per project is in this directory,
with a `.meta` file holding the commit and toolchain.

- Toolchain: rustc 1.100.0, host `aarch64-apple-darwin` (macOS, Apple silicon).
  Cargo 1.100 uses the build-dir v2 layout.
- Measured on 14 Nov 2026.
- Reproduce: `TOOLCHAIN=1.100.0 bench/run.sh <work-dir> <out-dir>`, then
  `python3 bench/table.py <out-dir>`.

| Project | Commit | `target/` | Debuginfo | Incremental | Metadata | of which twice | Code |
|---|---|--:|--:|--:|--:|--:|--:|
| clap | `ec93e71` | 81 MB | 29% | 42% | 7% | 4% | 21% |
| fd | `6cf3302` | 183 MB | 2% | 9% | 48% | 24% | 41% |
| hyperfine | `c516152` | 264 MB | 19% | 8% | 47% | 24% | 26% |
| axum | `3ebdbe6` | 266 MB | 19% | 26% | 36% | 17% | 17% |
| tokio | `f3e855f` | 270 MB | 27% | 32% | 24% | 10% | 16% |
| ripgrep | `178cc02` | 307 MB | 29% | 22% | 20% | 10% | 29% |
| alacritty | `aa5d942` | 715 MB | 25% | 15% | 31% | 15% | 28% |
| bat | `8cf33bd` | 990 MB | 31% | 4% | 29% | 15% | 34% |
| tantivy | `2ad36b4` | 1.24 GB | 35% | 21% | 19% | 9% | 24% |
| zola | `48d4e7a` | 1.95 GB | 0% | 9% | 48% | 24% | 40% |
| starship | `d128ba8` | 2.31 GB | 36% | 8% | 25% | 12% | 30% |
| cargo | `382ecb8` | 3.17 GB | 38% | 13% | 17% | 8% | 30% |
| rust-analyzer | `114dcb8` | 4.39 GB | 22% | 29% | 11% | 5% | 38% |
| typst | `84cca40` | 5.24 GB | 54% | 12% | 17% | 8% | 16% |
| uv | `d5c3f17` | 5.90 GB | 20% | 27% | 14% | 6% | 38% |
| ruff | `b140af2` | 6.28 GB | 22% | 32% | 14% | 6% | 31% |
| nushell | `3e2dc65` | 6.54 GB | 36% | 17% | 17% | 9% | 28% |
| wasmtime | `c52bc6d` | 7.45 GB | 36% | 27% | 11% | 6% | 25% |
| **all 18** | | **47.5 GB** | **31%** | **21%** | **17%** | **8%** | **30%** |

## Reading the table

- Percentages are shares of the project's `target/` on disk. Debuginfo,
  incremental, metadata and code add up to nearly 100%; the rest is small
  files such as fingerprints and dep-info.
- "Metadata" is crate metadata in `.rmeta` files plus the copy inside each
  `.rlib`. "Of which twice" is the part that exists in both places.
- "Incremental" is what deleting `incremental/` would free. Files that rustc
  hard-links between the artifacts and `incremental/` count as the artifact.

## What stands out

- Over the 18 projects, 31% of the bytes are debuginfo, 21% the incremental
  cache, 17% metadata, and 8% is metadata stored a second time.
- Projects that turn debuginfo down in their own `dev` profile show it:
  zola sets `debug = 0` (0% debuginfo) and fd sets `line-tables-only` with no
  debuginfo for dependencies (2%). In both, metadata becomes the largest part,
  at 48%.
- typst is the other extreme: 54% of 5.24 GB is debuginfo.

## Against Rust 1.99.1

The same commits were measured with rustc 1.99.1 and the classic layout in
[`../2026-11-14`](../2026-11-14). The 17 projects common to both runs take
44.4 GB with 1.100 against 44.9 GB with 1.99.1 (1.2% less), and the shares
move by at most two points. cargo itself needs rustc 1.100, so it is only in
this run.

## Limits

- One platform. On macOS debuginfo stays in loose object files next to the
  `.rlib`; on Linux it is inside the `.rlib`, so the split by file kind
  differs there. The split by content should be comparable, but it has not
  been measured on these projects.
- One build per project, default members of each workspace, no tests.
