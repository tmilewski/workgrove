# CLI integration tests

[tests/cli.rs](../tests/cli.rs) runs the built executable via `CARGO_BIN_EXE_workgrove` against real
temporary Git repositories and worktrees. Extend the existing `Fixture` and
helpers before introducing another harness.

- Keep state, configuration, repository files, and fake executables inside the
  fixture. Set environment overrides on each child `Command`, not globally;
  Cargo can run tests concurrently.
- Use repo-local Git identity settings. Never change the developer's global Git
  configuration, use their registry, or require their GitHub credentials.
- For remote/PR scenarios, follow the local bare remote plus PATH-scoped `git`
  and `gh` wrappers in `merged_github_pr_with_exact_head_allows_cleanup`.
  Test network failures through fixtures rather than contacting GitHub.
- Assert observable results: exit status, parsed JSON, file preservation,
  worktree registration, and branch retention where relevant. For a cleanup
  blocker, verify that removal leaves the affected work intact, not just that
  an error string appears.
- Cover successful behavior as well as refusal/recovery when changing lifecycle
  safety. Exercise partial progress and repeated `resume` when changing saved
  state or hooks.
- Keep the test process outside the workspace under inspection. Real `lsof`
  checks are part of the behavior; do not globally stub them to make tests pass.
  Clean up any child processes a test starts.

Run one case with `cargo test --locked --test cli TEST_NAME`, then the applicable
[development validation commands](development.md#validation). Tests must work on both macOS and Linux.

For manual lifecycle checks, set `WORKGROVE_STATE_DIR` and `XDG_CONFIG_HOME` to
temporary directories. Ancestor `.workgrove.json` files can still apply. Do not
clean the development checkout or its parent workspace to make a test pass;
build outputs themselves can block cleanup.
