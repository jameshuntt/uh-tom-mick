# uh-tom-mick

Atomic-state library family built on Big Bad Bool.

Workspace crates:
- `uh-tom-mick-core`
- `uh-tom-mick-derive`
- `uh-tom-mick`

## Releasing

Bump `version` in Cargo.toml, add a dated `## <version> (YYYY-MM-DD)` entry to CHANGELOG.md, and push to
`main`. CI (.github/workflows/ci.yml) runs the tests and clippy, then publishes to crates.io each crate whose
version is not there yet, in dependency order.
