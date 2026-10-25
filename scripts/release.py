"""Single-crate Changesets-style versioning. Requires Python 3.11+."""

import argparse
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


BUMP_ORDER = ("patch", "minor", "major")


def version_numbers(value):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
        raise ValueError(f"Expected a stable major.minor.patch version, got {value!r}")
    return tuple(map(int, value.split(".")))


def version(root):
    value = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    version_numbers(value)
    return value


def changesets(root):
    result = []
    for path in sorted((root / ".changeset").glob("*.md")):
        if path.name == "README.md":
            continue
        text = path.read_text()
        empty = re.fullmatch(r"---\n[ \t\n]*---(?:\n([\s\S]*))?", text)
        if empty:
            result.append((path, None, (empty[1] or "").strip()))
            continue
        match = re.fullmatch(
            r'---\n(?:"workgrove"|\'workgrove\'|workgrove): (patch|minor|major)\n---\n\s*(\S[\s\S]*)',
            text,
        )
        if not match:
            raise ValueError(f"Invalid changeset: {path}; see .changeset/README.md")
        result.append((path, match[1], match[2].strip()))
    return result


def replace_version(text, header, old, new):
    blocks = re.split(r"(?=^\[)", text, flags=re.MULTILINE)
    matches = 0
    for index, block in enumerate(blocks):
        if not block.startswith(header + "\n"):
            continue
        parsed = tomllib.loads(block)["package"]
        package = parsed[0] if isinstance(parsed, list) else parsed
        if package.get("name") != "workgrove":
            continue
        if package["version"] != old:
            raise ValueError("Cargo.toml and Cargo.lock versions disagree")
        blocks[index], count = re.subn(
            r'^version = "' + re.escape(old) + r'"$',
            f'version = "{new}"', block, flags=re.MULTILINE,
        )
        matches += count
    if matches != 1:
        raise ValueError("Expected exactly one workgrove package version")
    return "".join(blocks)


def prepare(root, requested_version=None):
    pending = changesets(root)
    bumps = [level for _, level, _ in pending if level is not None]
    old = version(root)
    if requested_version is not None:
        if version_numbers(requested_version) <= version_numbers(old):
            raise ValueError(f"Requested version must be greater than {old}")
        new = requested_version
    else:
        if not bumps:
            raise ValueError("No changesets requesting a release")
        numbers = list(version_numbers(old))
        bump = max(bumps, key=BUMP_ORDER.index)
        index = 2 - BUMP_ORDER.index(bump)
        numbers[index] += 1
        numbers[index + 1:] = [0] * (2 - index)
        new = ".".join(map(str, numbers))
    entry = f"## {new}\n"
    if not bumps:
        entry += "\nRelease maintenance; no end-user CLI changes.\n"
    for kind in reversed(BUMP_ORDER):
        summaries = [summary for _, level, summary in pending if level == kind]
        if summaries:
            entry += f"\n### {kind.title()} Changes\n\n"
            entry += "\n\n".join("- " + summary.replace("\n", "\n  ") for summary in summaries) + "\n"
    changelog = root / "CHANGELOG.md"
    previous = changelog.read_text() if changelog.exists() else "# Changelog\n"
    if not previous.startswith("# Changelog\n"):
        raise ValueError("CHANGELOG.md must start with '# Changelog'")
    updates = {
        root / "Cargo.toml": replace_version((root / "Cargo.toml").read_text(), "[package]", old, new),
        root / "Cargo.lock": replace_version((root / "Cargo.lock").read_text(), "[[package]]", old, new),
        changelog: "# Changelog\n\n" + entry + "\n" + previous.removeprefix("# Changelog\n").lstrip(),
    }
    for path, contents in updates.items():
        path.write_text(contents)
    for path, _, _ in pending:
        path.unlink()
    return new


def notes(root):
    current = version(root)
    changelog = (root / "CHANGELOG.md").read_text()
    match = re.search(r"^## " + re.escape(current) + r"\n(.*?)(?=^## |\Z)", changelog, re.MULTILINE | re.DOTALL)
    if not match or not match[1].strip():
        raise ValueError(f"No changelog entry for {current}")
    return match[1].strip()


def check_pr(root, base):
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=root, text=True)

    base = git("merge-base", base, "HEAD").strip()
    added = set(git("diff", "--name-only", "--diff-filter=A", base, "HEAD").splitlines())
    if any(path.relative_to(root).as_posix() in added for path, _, _ in changesets(root)):
        return

    # A release PR consumes changesets instead of adding one. Reproduce it exactly.
    paths = git("ls-tree", "-r", "--name-only", base).splitlines()
    inputs = {
        path: git("show", f"{base}:{path}")
        for path in paths
        if path in ("Cargo.toml", "Cargo.lock", "CHANGELOG.md")
        or (Path(path).parent == Path(".changeset") and path.endswith(".md"))
    }
    with tempfile.TemporaryDirectory() as directory:
        expected = Path(directory)
        (expected / ".changeset").mkdir()
        for path, contents in inputs.items():
            (expected / path).write_text(contents)
        if version_numbers(version(root)) > version_numbers(version(expected)):
            prepare(expected, requested_version=version(root))
            outputs = {
                path.relative_to(expected).as_posix(): path.read_text()
                for path in expected.rglob("*") if path.is_file()
            }
            expected_changes = {
                path for path in inputs.keys() | outputs.keys()
                if inputs.get(path) != outputs.get(path)
            }
            actual_changes = set(git("diff", "--name-only", base, "HEAD").splitlines())
            if actual_changes == expected_changes and all(
                ((root / path).read_text() if (root / path).is_file() else None) == outputs.get(path)
                for path in expected_changes
            ):
                return
    raise ValueError("PR must add a valid .changeset/*.md file, or contain only the generated release changes")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "prepare", "version", "notes"))
    parser.add_argument("--base", help="For check: require a new changeset relative to this Git ref")
    parser.add_argument("--version", help="For prepare: use this stable version instead of the changeset bump")
    args = parser.parse_args()
    if args.version is not None and args.command != "prepare":
        parser.error("--version is only supported with prepare")
    root = Path(__file__).resolve().parents[1]
    try:
        if args.command == "check":
            current = version(root)
            replace_version((root / "Cargo.lock").read_text(), "[[package]]", current, current)
            if args.base:
                check_pr(root, args.base)
            print(f"Validated {len(changesets(root))} changeset(s)")
        elif args.command == "prepare":
            print(prepare(root, requested_version=args.version))
        else:
            print({"version": version, "notes": notes}[args.command](root))
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
