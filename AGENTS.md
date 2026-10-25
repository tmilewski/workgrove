# Workgrove

A Rust CLI for creating and safely retiring groups of Git worktrees.
Read the relevant guide before changing code:

- [README](README.md): user-facing behavior and supported workflows.
- [Development](docs/development.md): code map, prerequisites, and validation commands.
- [Implementation](docs/implementation.md): lifecycle, cleanup safety, settings, and CLI contracts.
- [Testing](docs/testing.md): isolated fixtures and behavior verification.
- [Releases](docs/releases.md): changesets, release tooling, and CI guardrails.

Preserve user edits and conservative cleanup: failed or incomplete checks must
block removal. Never bypass blockers by forcing removal, deleting branches or
user files, or terminating processes. Exercise lifecycle operations on disposable
fixtures unless the task explicitly concerns a real workspace.

Update the relevant `docs/` guide in the same change as code, workflows, or new
requirements. Remove stale guidance, keep links current, and reconcile related
README instructions. Keep this file short; put details in focused guides.

Every ordinary PR needs a new changeset. Use a version bump for user-facing
changes and empty frontmatter for non-user-facing changes; see the release guide.
Run the relevant validation and report what passed or could not run.
