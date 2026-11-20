# Contributing

Thanks for looking. This is a small tool with one job: say where the bytes
Rust keeps on disk go, starting with a Cargo target directory, in numbers that
can be checked.

## How changes get in

- `main` is protected. Every change goes through a pull request, and CI has
  to pass before it is merged. That includes the maintainer's own changes.
- One pull request per change. Small ones are reviewed faster.
- If the change is visible to users, add a line to `CHANGELOG.md` under
  "Unreleased".
- For anything bigger than a fix, open an issue first so the direction can be
  agreed before you write the code.

## Running the checks

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same three on Linux and macOS with stable Rust, and on Linux with
the minimum supported Rust version (`rust-version` in `Cargo.toml`).
A separate job runs `tests/layouts.rs` with Cargo 1.100.

The tests drive the compiled binary as a black box:

- `tests/du.rs` builds the small workspace in `tests/fixture` for real and
  measures it, including `--what-if`. It takes a few seconds.
- `tests/measure.rs` and `tests/property.rs` build target directories by hand,
  in both layouts, with hard links and symlinks, and check that every view
  adds up to the total and the total to `du`.
- `tests/layouts.rs` builds the fixture with a real Cargo 1.100 and checks
  the build-dir v2 layout. It runs when the 1.100.0 toolchain is installed
  and skips otherwise; the `layout-v2` CI job installs it, so there it cannot
  skip.
- `tests/disk.rs` lays out Cargo and rustup homes by hand and checks
  `cargo heft disk`.
- `tests/cli.rs` pins the command line: flags, errors and exit codes.
- Unit tests in `src/main.rs` pin how single paths are classified.

The fixture's manifests are stored as `Cargo.toml.in`, so Cargo packages
the tests with the crate (it leaves out any directory with a `Cargo.toml`);
`fixture()` copies the workspace to a scratch directory and names them back.
Edit the `.in` files. `tests/common` has the helpers; `assert_consistent` is the one to call on
any new JSON report. The fixture has a library, a binary and a build script
on purpose: between them they produce every kind of artifact the tool
classifies.

## When a change moves a number

The tests check two things that must stay true:

- the total equals what `du` reports for the same directory;
- the views (by kind, by content) each add up to that total.

If you change how something is classified or counted, show a before and
after on a real target directory in the pull request. Where you can, check
the new number against an independent tool (`du`, `size`, `ar`,
`llvm-size`) and say which one.

Two things that are easy to get wrong:

- rustc hard-links files between the compiled artifacts and `incremental/`.
  Each inode is counted once, as the artifact, so "incremental" means what
  deleting that directory would free.
- Cargo has two layouts. Up to 1.99 artifacts are in `deps/`; from 1.100
  they are in `build/<pkg>/<hash>/out/`. Classification has to hold for both,
  and `cargo +<version> test` is the way to check an older one.

## Benchmark data

`bench/run.sh` builds a list of well-known projects from scratch and measures
each one; `bench/table.py` turns a results directory into the table in its
README. The "Benchmark (Linux)" workflow runs the same script on GitHub's
Linux runners, one job per project, when started by hand from the Actions
tab; its results are downloaded and committed like the others. Results are
committed under `bench/results/<date>...` with the commit
of every project and the toolchain, so a number can be reproduced. Add new
runs next to the old ones instead of overwriting them.

## Releases

The maintainer bumps the version in `Cargo.toml`, moves "Unreleased" in
`CHANGELOG.md` under the new version, merges that pull request, and pushes a
`v<version>` tag. The release workflow publishes to crates.io and creates the
GitHub release from the changelog section.

## Licence

Contributions are accepted under the same terms as the project: MIT or
Apache-2.0, at the user's option.
