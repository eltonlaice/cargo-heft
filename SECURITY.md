# Security

cargo-heft reads files in a target directory and, with `--what-if`, runs
`cargo build` on the workspace you point it at. It does not send anything
over the network.

If you find a security problem, please do not open a public issue. Report it
privately through
[GitHub's security advisory form](https://github.com/eltonlaice/cargo-heft/security/advisories/new).
Only the latest released version is supported.
