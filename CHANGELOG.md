# Changelog

## 0.1.1 (2026-10-16): uh-tom-mick uh-tom-mick-core uh-tom-mick-derive

- The published 0.1.0 packages had `repository = "https://github.com/jameshuntt"`, the GitHub profile, so the crates.io page linked nowhere useful. 0.1.1 points at this repo. The workspace manifest here already said so: [Cargo.toml @ 6d7f574](https://github.com/jameshuntt/uh-tom-mick/blob/6d7f574d3a5a553d6b19312bec9f41db5accc793/Cargo.toml).
- No code change. Each 0.1.1 package was built from its published 0.1.0 package with only `version` and `repository` changed, and `diff -r` of each 0.1.0 package's `src` against `crates/<name>/src` here prints nothing.
