# Workgrove

Create one isolated working folder spanning several independent Git repositories.
Keep separate branches and PRs, then retire the group after checking every checkout.
Workgrove is a standalone tool; Orca or another editor can invoke it.

Requires macOS or Linux, Rust, Git, and `lsof`. GitHub repositories also
require an authenticated `gh` CLI with access to their pull requests. Cargo downloads
the Rust crate dependencies on the first build.

## Run it

Build the executable, then run it directly:

```sh
cargo build --release
./target/release/workgrove --help
```

The examples below assume `workgrove` is on your PATH. Otherwise, use the executable's full path.

```sh
workgrove create NAME \
  --source /path/to/repositories \
  --destination /path/to/workspaces/NAME
```

Workgrove discovers immediate child Git repositories. It creates a
`workgrove/NAME` branch in each and checks them out together:

```text
workspaces/NAME/
  REPOSITORY_A/
  REPOSITORY_B/
  ...
```

The source checkouts stay on their existing branches. Their uncommitted changes
are not copied. Branches and Git objects are shared with the source repositories,
as with ordinary Git worktrees. The destination must not already exist.

With an origin remote, creation fetches it and uses the locally configured
`origin/HEAD` as the base. Without remotes, it uses the source's current branch.
If `origin/HEAD` is missing, specify the base explicitly. Select a subset or override
individual bases with repeatable flags:

```sh
workgrove create NAME \
  --source /path/to/repositories --destination /path/to/workspaces/NAME \
  --repo REPOSITORY_A --repo REPOSITORY_B \
  --base REPOSITORY_A=BASE_REF --base REPOSITORY_B=BASE_REF
```

## Inspect and remove

Run lifecycle commands from outside the workspace. A shell whose working directory
is inside the workspace will correctly block removal.

```sh
workgrove list
workgrove status NAME
workgrove check NAME
workgrove remove NAME
```

`status` reports local state without network or process checks. It never certifies
removal safety. `check` fetches origin, queries GitHub PRs, checks local files and
processes, and reports `safe_to_remove`. It does not remove files; fetching does
update remote-tracking refs.

`remove` checks the whole group first, then rechecks each repository immediately
before removing it with Git. It blocks on:

- Modified, staged, untracked **or ignored** files. This includes build outputs,
  dependencies, secrets, and local configuration: decide what to retain yourself.
- Files outside managed repositories in the workspace root.
- A detached HEAD, changed checkout branch, changed remote configuration, or an
  in-progress merge, rebase, cherry-pick, revert, or bisect.
- Commits not contained in the configured base, unless a merged GitHub PR has
  exactly the checkout's current head SHA. This supports squash merges without
  treating later local commits as merged.
- Unpublished local commits not otherwise covered by that exact merged PR.
- Open PRs or closed-but-unmerged PRs for the workspace branch, including drafts.
- Failed fetches, unavailable PR metadata, unsupported providers, or incomplete
  process inspection.
- Processes visible to `lsof` with open files or working directories in the workspace.

Workgrove never force-removes a worktree or deletes branches. It leaves source
checkouts, repository stashes, local branches, and remote branches intact. Stashes
are shared repository state, not files owned by an individual worktree.

An abandoned, unmerged feature intentionally remains blocked. This version has no
automatic discard override. Preserve or dispose of such work manually using Git;
do not expect a closed PR to count as a merge.

## Recovery

```sh
workgrove resume NAME
```

Records live outside the workspace at `~/.local/state/workgrove`. Override this with
`WORKGROVE_STATE_DIR`. Use the same directory for every invocation, including Orca
hooks. Records are written atomically before and after worktree mutations, and a
process lock prevents concurrent Workgrove commands from modifying the registry.

If creation fails halfway through, fix the reported cause and run `resume`. Existing
worktrees are verified and retained. If removal fails halfway through, `resume`
continues cleanup after checking what remains. Creation records are retained on
failure rather than rolling back and risking files written by hooks.

Completed records remain for inspection, so workspace names cannot be reused.
The retained local branches would also conflict with reuse. Use distinct task names.

## Calling from Orca

Have Orca or an agent invoke this sequence:

1. Run `workgrove --json create NAME --source SOURCE --destination DESTINATION`.
2. Only on exit 0, open `DESTINATION` as the Orca folder workspace and launch agents there.
3. To retire it, stop its agents, servers, and terminals, then call
   `workgrove --json remove NAME` from outside `DESTINATION`.
4. Only on exit 0, remove the corresponding Orca workspace entry. On failure, show
   the report and retain the entry so the user can resolve blockers.

Workgrove owns these Git worktrees. Do not separately remove them through Orca's
per-repository worktree removal command, which may also delete branches.

The CLI is ready to call, but installing it does **not** attach callbacks to Orca's
folder creation or close buttons. That requires an Orca lifecycle integration.
An archive hook can be a caller, but it must run outside the folder and propagate
Workgrove's nonzero exit status; any path that bypasses the hook also bypasses
these checks. Closing a tab alone should simply retain the workspace.

Every successful command emits JSON. `--json` also makes errors JSON and must
precede the subcommand. Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Command succeeded (`status` is informational, not a safety approval) |
| 1 | Command or verification operation failed |
| 2 | `check`, `remove`, or cleanup `resume` found blockers; also argparse usage errors |

## Scope and limits

- Immediate child repos only; no submodules, bare repos, symlinked repo roots,
  nested discovery, or multiple checkouts of the same repository.
- Exactly one `origin` remote or no remotes. GitHub.com PR verification supports
  standard HTTPS, SCP-style SSH, and `ssh://git@github.com/` URLs. GitLab, GitHub
  Enterprise, SSH aliases, multiple-remotes/fork workflows, and PRs targeting
  another repository need additional provider support. Unsupported remotes can
  be used to create workspaces, but block automatic removal.
- Source repositories must remain at their recorded locations.
- No automatic editor registration, process termination, dependency installation,
  copying of root documents, or copying of environment files.
- The process check covers what `lsof` can observe for the current user. The lock
  coordinates Workgrove commands, not editors or external Git commands. Stop all
  writers before removal: checks cannot prevent another program from writing a
  file immediately afterward. This is conservative cleanup, not a filesystem
  transaction or a guarantee against concurrent external writes.
- Remote PR state is checked at cleanup time; it is not locked against concurrent
  remote changes. Linked issues, CI runs, and editor chat histories are not
  inspected. Keep editor metadata outside the removed folder.

## Development

```sh
cargo test
cargo build --release
```

Tests create real temporary Git repositories and worktrees, exercise cleanup
blockers and recovery, and use a local bare Git remote with a mocked `gh` response
to cover merged pull request verification.
