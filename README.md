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

## Settings

Global settings live at `~/.config/workgrove/config.json` (or
`$XDG_CONFIG_HOME/workgrove/config.json`). Folder and repo settings live in
`.workgrove.json`. Workgrove searches from the source directory up to the
filesystem root, and does the same for each selected repo.

A global or root-folder file can configure several directories:

```json
{
  "~/Code/Workgrove": {
    "workspace_destination": "./.workgrove/trees",
    "include": ["./api", "./web"],
    "exclude": [],
    "directories": {
      "./api": { "after_create": ["pnpm install"] },
      "./web": { "after_create": ["pnpm install", "pnpm build"] }
    }
  },
  "~/Code/AnotherProject": {
    "workspace_destination": "~/worktrees/another-project"
  }
}
```

A repo's `.workgrove.json` can contain the settings directly. This flat form also
works in a root folder and applies to that folder and its descendants:

```json
{
  "workspace_destination": "./.workgrove/trees",
  "after_create": ["pnpm install"]
}
```

Profiles apply to the named directory and its descendants. More-specific
profiles override broader profiles, field by field. For the same directory,
folder files override the global file, and files closer to the source override
ancestor files. Arrays **replace** inherited arrays; an explicit `[]` clears
an inherited value. Omitted fields inherit. Nested `directories` entries must
point below their containing profile.

Relative profile keys are resolved from the settings file; nested keys and
settings paths are resolved from their profile directory. In a flat file,
relative paths are resolved from the file's directory. `~` expands to your home.
These paths do not depend on the shell's current directory.

| Setting | Default and behavior |
| --- | --- |
| `workspace_destination` | `<source>/.workgrove/trees`; the workspace is created at `<workspace_destination>/<name>` |
| `include` | Discover immediate child Git repos; when specified, use exactly these repo paths |
| `exclude` | `[]`; omit these paths from discovery or `include` |
| `after_create` | `[]`; shell commands run in each new repo worktree after all worktrees are created |
| `after_remove` | `[]`; shell commands run in each source repo after the workspace is removed |

`include` and `exclude` use directory paths, not globs. Selected repos must be
immediate children of the source folder. `--repo` replaces both filters;
`--destination` sets the exact destination and overrides the configured parent.
Destination and repo selection are resolved for the source folder; child repo
profiles control that repo's hooks, not the location of the shared workspace.

`--source` defaults to the current directory. When the source is itself a Git
repo, Workgrove creates a single-repo workspace. Both single-repo and multi-repo
workspaces retain the layout `<destination>/<repo-name>`.

```sh
workgrove create task-name
```

Hooks support `{name}`, `{workspace}`, and `{repo}` substitutions. Resolved hooks
are saved in the recovery record, so config edits affect future workspaces.
Completed hook lists are skipped on recovery; if a list fails or is interrupted,
`resume` retries that repo's list from the beginning. Commands should therefore
be safe to repeat. Hook output goes to stderr to keep stdout valid JSON.
Dependency installs can create ignored files that block removal; `after_remove`
runs only after removal succeeds and cannot clean up those blockers beforehand.

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

- A source repo or its immediate child repos only; no submodules, bare repos, symlinked repo roots,
  nested discovery, or multiple checkouts of the same repository.
- Exactly one `origin` remote or no remotes. GitHub.com PR verification supports
  standard HTTPS, SCP-style SSH, and `ssh://git@github.com/` URLs. GitLab, GitHub
  Enterprise, SSH aliases, multiple-remotes/fork workflows, and PRs targeting
  another repository need additional provider support. Unsupported remotes can
  be used to create workspaces, but block automatic removal.
- Source repositories must remain at their recorded locations.
- No automatic editor registration or process termination. Dependency installation
  and file copying require explicitly configured hooks.
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
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
python3 scripts/release.py check
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Tests create real temporary Git repositories and worktrees, exercise cleanup
blockers and recovery, and use a local bare Git remote with a mocked `gh` response
to cover merged pull request verification.

PR CI runs **Check Formatting** and **Check Changesets** in parallel, then starts
**Run Test Matrix** on Linux and macOS only after both pass and Rust source,
Cargo files, test fixtures, launcher scripts, or build/CI configuration changes.
Documentation-only PRs skip the Rust matrix. Rust dependencies and build outputs
are cached per platform and build mode; Linux installs `lsof` only if missing.
New PR commits cancel superseded CI runs for that PR.

A separate
**Test Changeset Script** job runs after those checks only when
`scripts/release.py` or `scripts/test_release.py` changes. For PRs, it checks the
whole PR diff; for pushes, the pushed commit range; for manual runs, the branch
diff against `main` (or the latest commit when running on `main`). Release
tooling requires Python 3.11+.
PRs run Clippy and tests without an optimized build. Manual **Publish release**
always runs the full Rust matrix and optimized builds, regardless of changed files.
Every ordinary PR must add a new [changeset](.changeset/README.md): request a
version bump for user-facing changes, or use empty frontmatter for changes with
no effect on the end-user CLI. The **Create release** workflow turns pending changesets into a version and changelog PR;
review and merge it manually, then manually run **Publish release**. Merging
alone never publishes. External contributors' CI requires maintainer approval
through the repository settings documented in the changeset guide, including
the setting to apply when making the repository public.
