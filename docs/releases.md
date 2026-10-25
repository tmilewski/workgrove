# Release tooling

[scripts/release.py](../scripts/release.py) is a Python 3.11+ standard-library implementation of single-crate
changesets. [scripts/test_release.py](../scripts/test_release.py) uses temporary repositories to test it. The format
and release procedure are defined in [the changeset guide](../.changeset/README.md).

- Keep validation commands non-mutating. `prepare` is mutating: it updates
  `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md`, then deletes consumed changesets.
  Preview it only in a disposable checkout unless release preparation is the task.
- Validate all input changesets and Cargo version consistency before writes.
  Only update Workgrove's lockfile entry; preserve dependency versions and prior
  changelog entries.
- Require changesets only for user-facing changes. Documentation, CI, tests,
  and internal refactoring with no CLI behavior change do not need one; do not
  create an empty changeset solely for those changes.
- Existing empty changesets remain supported by the release script and do not
  request a version bump. The highest requested bump wins. Preserve failure
  without writes when no changeset requests a release.
- `check --base REF` compares committed history against the merge base with HEAD;
  it is not a check of uncommitted new files. The release-PR exception must match
  generated release output exactly, never just a branch name or label.

## Current validation limitation

`scripts/release.py check --base REF` still requires every ordinary PR to add a
changeset, and CI invokes that check. This predates the policy above; a
non-user-facing PR without a changeset can therefore fail that check. Updating
that enforcement is separate from maintaining these guides. Do not describe the
current validator as already implementing the exemption.

For release tooling changes, run from the repository root:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/release.py check
```

## Workflow guardrails

- Keep ordinary PR CI on `pull_request` with read-only permissions and without
  repository secrets. Contributor approval is a repository setting; a workflow
  step cannot enforce approval before its own execution.
- Keep release creation and publication manual and restricted to `main`.
  Merging a PR must not publish or auto-approve/auto-merge a release PR.
- Preserve the full Rust matrix and optimized builds before publication. When
  adding build inputs, update CI change detection so relevant PRs run the matrix.
- Retrying publication must check that an existing version tag points to the
  same commit and skip an already published release.
- Test workflow/script changes without dispatching publication or pushing tags.
  Follow explicit task authorization for actual releases.

User-facing changes need a new, uniquely named `.changeset/*.md` describing their
effect and requesting the appropriate version bump. Do not reuse a pending
changeset or manually bump the package version for ordinary changes. Release
preparation updates both Cargo files and the changelog. Publishing, tagging, and
merging are separate actions, not validation steps.
