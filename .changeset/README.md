# Changesets

Every ordinary PR must add a uniquely named Markdown file here, such as
`.changeset/clear-cleanup-errors.md`:

```markdown
---
"workgrove": patch
---

Explain the change and its effect on users. This text becomes the release notes.
```

Use `patch` for fixes, `minor` for new features, or `major` for breaking changes.
The highest requested bump wins; several patch changesets produce one patch bump.
Minor and major bumps reset the lower components. This also applies before 1.0:
a `major` changeset for 0.1.0 produces 1.0.0.

For a PR with no effect on the end-user CLI, such as CI, documentation, or
test-only work, add an **empty changeset** instead:

```markdown
---
---
```

An optional explanation can follow the empty frontmatter. Empty changesets
satisfy PR CI but do not bump the version or appear in the changelog. They are
removed alongside the other changesets when preparing the next real release.
If only empty changesets exist, release preparation stops without changing files.
A zero-byte file is not a valid changeset; use the two delimiters shown above.

This is a single-crate implementation of the [Changesets](https://changesets.dev/)
approach, using Python 3.11+ without Node or npm. The supported frontmatter is
either empty or exactly one `workgrove: patch|minor|major` line as shown above
(the package name may be quoted). Prereleases, multiple packages, and general
YAML are not supported. CI checks that the PR
adds its own valid file; an existing changeset on `main` does not satisfy it.
The only exception is a release PR whose entire diff exactly matches the output
of consuming the base branch's changesets. A branch name or label cannot bypass
the check.

Run `python3 scripts/release.py check` to validate pending files. To preview a
release in a disposable checkout, run `python3 scripts/release.py prepare`. It
updates Cargo.toml, Cargo.lock, and CHANGELOG.md, and removes consumed changesets.
Run `python3 scripts/release.py check --base origin/main` to check a committed PR.

## Releasing

1. Merge the desired changesets into `main`.
2. Run **Create release** in GitHub Actions on `main`. It creates or updates the
   `release/next` PR with the version and changelog changes. Without a changeset
   requesting a version bump, the workflow fails without creating a PR.
3. Review the release PR and merge after CI passes. Keep this branch generated;
   rerunning the workflow regenerates it from `main`.
4. Manually run **Publish release** on `main` when ready to publish. Merging a PR
   never publishes a release. The workflow runs CI on the selected commit, then
   creates `vX.Y.Z` and a GitHub release using that version's changelog entry.
   Releases contain GitHub's
   source archives; this workflow does not publish to crates.io or attach binaries.

To let **Create release** open its PR, GitHub requires **Settings → Actions →
General → Allow GitHub Actions to create and approve pull requests**. This is a
bundled GitHub permission, not an automatic approval rule: these workflows never
submit reviews, approve workflow runs, enable auto-merge, or merge PRs. Review and
merge the release PR manually. If you keep the bundled permission disabled,
prepare the release locally with `python3 scripts/release.py prepare` and open
the PR yourself instead.

The workflows use the built-in `GITHUB_TOKEN`; no PAT is needed.
Create release explicitly dispatches CI on the generated branch
because PRs created with that token do not automatically trigger PR workflows.

If publishing fails, rerun the failed workflow on the original release commit.
An already published release is skipped. An existing tag must point to that
exact commit. Both release workflows are manual-only and run only on `main`.

## Contributor approval

CI uses `pull_request` with a read-only token and no repository secrets. GitHub
must enforce approval before starting a contributor's workflow; this cannot be
enforced by a step inside that same workflow.

Configure **Settings → Actions → General** for the repository's visibility:

- **Private:** enable fork PR workflows with **Require approval for fork pull
  request workflows**. Keep write tokens and secrets for fork workflows disabled.
- **Public:** under **Approval for running fork pull request workflows from
  contributors**, choose **Require approval for all external contributors**.
  Do this when making the repository public; GitHub rejects this policy's API
  endpoint while the repository is private. Do not select either first-time-only
  policy, since returning external contributors must also need approval.

After making the repository public, an administrator can apply and verify the
policy with:

```sh
gh api --method PUT repos/tmilewski/workgrove/actions/permissions/fork-pr-contributor-approval \
  -f approval_policy=all_external_contributors
gh api repos/tmilewski/workgrove/actions/permissions/fork-pr-contributor-approval
```

See [GitHub's workflow approval settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository).
