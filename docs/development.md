# Development

## Navigation

| Task | Start here |
| --- | --- |
| CLI arguments, JSON output, exit codes | `Cli`, `Cmd`, `main`, `fail` in `src/main.rs` |
| Creation, recovery, registry locking | `create`, `finish_create`, `save`, `load`, `lock` in `src/main.rs` |
| Cleanup safety and GitHub verification | `verify`, `review_state`, `inspect_repo`, `inspect`, `process_blockers`, `remove` in `src/main.rs` |
| Configuration inheritance and path resolution | `src/settings.rs` |
| End-to-end CLI behavior | `tests/cli.rs` |
| Development launcher | `bin/workgrove` |
| Versioning and changeset validation | `scripts/release.py`, `scripts/test_release.py`, `.changeset/README.md` |
| CI and manual release workflows | `.github/workflows/` |


## Validation

Run commands from the repository root. Development requires stable Rust with
edition 2024 support, Git, and `lsof` on macOS or Linux. Release tooling requires
Python 3.11+. Tests mock GitHub access; real GitHub operations require `gh` auth.

For Rust, CLI, or launcher changes:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

For release tooling changes, also run:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
```

For every change, validate pending changesets:

```sh
python3 scripts/release.py check
```

Use `cargo build --locked --release` when checking optimized builds or the
release launcher. `bin/workgrove` prefers an existing release binary, which can
be stale; use `cargo run --locked -- ...` or the integration tests for current
source behavior. Documentation-only changes do not need the Rust test matrix.
Report checks run and any unavailable prerequisites accurately.

