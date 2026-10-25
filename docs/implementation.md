# Rust implementation

The crate has one binary ([src/main.rs](../src/main.rs)) and a settings module
([src/settings.rs](../src/settings.rs)). Integration coverage lives in
[tests/cli.rs](../tests/cli.rs); see the [testing guide](testing.md).

## Lifecycle and persistence

- Follow the existing creation/removal state transitions and persist records
  before and after worktree mutations. Keep the registry lock alive for the
  command and retain atomic record replacement in `save`.
- Creation failures retain recovery records and existing worktrees. Do not add
  rollback that might delete files written by hooks or other processes.
- Resume must verify an existing checkout's identity and branch before reuse.
  Removal recovery reconciles recorded state with both the filesystem and Git's
  worktree registry; a missing directory alone is insufficient evidence.
- Records currently use schema version 1 and `serde_json::Value`. When changing
  fields, account for records written by earlier binaries, including partially
  completed operations. Do not silently reinterpret old records.
- Resolved hooks are stored in records. Completed per-repo hook lists are
  skipped; interrupted lists retry from the beginning. Preserve that behavior
  and keep hook output on stderr.

## Removal safety

- Preserve the group-wide preflight and the per-repository recheck immediately
  before `git worktree remove`, including process checks.
- Dirty, staged, untracked, and ignored files, extra workspace-root files,
  changed checkout identity/branch/remotes, and ongoing Git operations block
  cleanup. Preserve the checks documented in the README.
- A merged PR can cover commits only when its head SHA exactly matches the
  current checkout. Open and closed-unmerged PRs still block removal. Failed
  fetches, invalid/unavailable PR data, unsupported providers, and incomplete
  `lsof` inspection must remain blockers.
- Retain local/remote branches, source checkouts, stashes, and completed records.
  Remove worktrees through Git without force and remove the workspace root only
  when empty. Never replace these operations with recursive deletion.
- The registry lock does not exclude external Git commands or file writers.
  Do not describe inspection followed by removal as a filesystem transaction.

## Settings and command contracts

- More-specific directory profiles override broader ones field by field; for
  equal scopes, preserve file precedence. Arrays replace rather than append;
  omitted fields inherit and explicit empty arrays clear inherited values.
- Resolve relative settings paths against the containing file/profile, not the
  process working directory. Nested profiles must remain below their parent.
- Source settings select repositories and the shared destination; per-repo
  settings select hooks. `--repo` overrides filters and `--destination` is the
  exact destination, not its parent.
- Successful commands emit JSON on stdout. `--json` formats command errors as
  JSON on stderr. Preserve exit 0 for success, 1 for operational errors, and 2
  for reported cleanup blockers (Clap also uses 2 for usage errors).
- Pass ordinary subprocess arguments separately through `Command`. Shell
  evaluation belongs to explicitly configured lifecycle hooks.

Keep CLI behavior, README examples, and relevant tests aligned. Stored recovery
records and machine-readable output are compatibility surfaces. Workgrove remains
a standalone CLI; editor registration and lifecycle callbacks belong to callers
unless the task explicitly changes that scope.
