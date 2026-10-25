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
- Every ordinary PR needs its own new changeset. User-facing changes require a
  version bump; docs, CI, tests, and internal refactoring with unchanged CLI
  behavior require a file with empty frontmatter instead.
- Existing empty changesets remain supported by the release script and do not
  request a version bump. The highest requested bump wins. Preserve failure
  without writes when no changeset requests a release and no explicit version
  is supplied.
- `check --base REF` compares committed history against the merge base with HEAD;
  it is not a check of uncommitted new files. The release-PR exception must match
  generated release output exactly, never just a branch name or label.

## Non-user-facing changes

Add a uniquely named `.changeset/*.md` file with empty frontmatter:

```markdown
---
---
```

An optional explanation may follow. This file records that the PR was assessed
as having no user-facing impact; it does not bump the version or produce release
notes. A zero-byte file is invalid. `check --base REF` requires a new file in the
committed PR diff; an existing pending changeset does not satisfy another PR.
Only a PR containing exactly the generated release changes is exempt.

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
- Keep the OS-specific test matrix jobs present for every eligible CI run so
  required checks receive a result. For documentation-only changes, skip Rust
  setup and test steps rather than the matrix job: a job-level condition is
  evaluated before matrix expansion and leaves OS-specific checks missing.
  Release checks always run the full matrix and optimized builds.
- Retrying publication must check that an existing version tag points to the
  same commit and skip an already published release.
- Test workflow/script changes without dispatching publication or pushing tags.
  Follow explicit task authorization for actual releases.

User-facing changes need a new, uniquely named `.changeset/*.md` describing their
effect and requesting the appropriate version bump. Do not reuse a pending
changeset or manually bump the package version for ordinary changes. Release
preparation updates both Cargo files and the changelog. Publishing, tagging, and
merging are separate actions, not validation steps.
