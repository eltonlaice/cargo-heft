# How cargo-heft counts

## Sizes

Sizes are what files take on disk (allocated blocks), not their length, so
the total matches `du -s`. Units are decimal: 1 MB is 1,000,000 bytes. `du -h`
counts in units of 1024, which is why the first line of the output also gives
the total in MiB.

When the target directory is a symlink, it is measured where it points, and
the first line shows both paths. Compare with `du -sL link` or `du -s` on the
real path: `du -s link` counts only the link.

A directory that cannot be read stops the measurement with its path, rather
than giving a total that would be too small.

## Hard links

Every file is counted once, however many names it has. rustc hard-links
object files and metadata between the compiled artifacts and `incremental/`.
A file shared that way counts as the artifact, so "incremental" is what you
would free by deleting that directory.

## What is inside the files

The "by content" view opens `.rlib` archives, `.a` static libraries, object
files and binaries, and splits each file's size on disk into metadata,
debuginfo and the rest, in proportion to what it finds inside. It reads ELF,
Mach-O and COFF. Other formats, such as WebAssembly or LLVM bitcode, count
entirely as code.

"other" in that view is everything that holds no compiled code: fingerprints,
dep-info, build script output, documentation. The output lists which kinds it
contains.

## Kinds, profiles and crates

Files are classified by name. Both target layouts are understood: the classic
one (`deps/`, `.fingerprint/`) and the build-dir layout Cargo uses from 1.100
(`build/<package>/<hash>/`).

A directory counts as a profile if it looks like one (it has `build/` and
`deps/`, `incremental/` or `examples/`). Documentation and packaged crates
are listed as "(not in a profile)".

Crates are recognised from file and directory names, and a binary is listed
under its package (the `rg` binary under `ripgrep`). The per-crate table is an
attribution, not an exact account. The total and the other three views cover
every byte.

## Pointing at part of a target directory

`--target-dir target/debug` or `--target-dir target/doc` works: when the
directory sits inside a target directory, paths are read from its root, so
the profile and the kinds keep their names.

## What it does not do

- It does not delete anything. For cleaning there are `cargo clean`,
  [cargo-sweep](https://crates.io/crates/cargo-sweep) and
  [kondo](https://crates.io/crates/kondo).
- It does not support Windows. There the code would fall back to file
  lengths and could not detect hard links, so the numbers would be wrong.
- `--what-if` runs five full builds, so it takes five times as long as a
  clean build. `incremental = false` also makes rebuilds after an edit
  slower, so that saving is for CI more than for local work.
