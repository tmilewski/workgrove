from pathlib import Path
import tempfile
import subprocess
import tomllib
import unittest

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / ".changeset").mkdir()
        (self.root / ".changeset/README.md").write_text("Instructions, not a changeset.\n")
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "workgrove"\nversion = "0.1.0"\nedition = "2024"\n'
            '\n[dependencies]\nexample = "1"\n'
        )
        (self.root / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "example"\nversion = "0.1.0"\n'
            '\n[[package]]\nname = "workgrove"\nversion = "0.1.0"\n'
            'dependencies = ["example"]\n'
        )

    def add_changeset(self, name, bump, summary):
        path = self.root / ".changeset" / f"{name}.md"
        path.write_text(f'---\n"workgrove": {bump}\n---\n\n{summary}\n')
        return path

    def test_highest_bump_updates_only_our_crate_and_preserves_history(self):
        (self.root / "CHANGELOG.md").write_text("# Changelog\n\n## 0.1.0\n\nOld notes.\n")
        self.add_changeset("fix", "patch", "Fix cleanup.\n\nPreserve user files.")
        self.add_changeset("feature", "minor", "Add a command.")
        self.assertEqual(release.prepare(self.root), "0.2.0")
        self.assertEqual(release.version(self.root), "0.2.0")
        lock = tomllib.loads((self.root / "Cargo.lock").read_text())
        self.assertEqual([p["version"] for p in lock["package"]], ["0.1.0", "0.2.0"])
        notes = release.notes(self.root)
        self.assertIn("### Minor Changes\n\n- Add a command.", notes)
        self.assertIn("- Fix cleanup.\n  \n  Preserve user files.", notes)
        self.assertNotIn("Old notes", notes)
        self.assertIn("## 0.1.0\n\nOld notes.", (self.root / "CHANGELOG.md").read_text())
        self.assertEqual(release.changesets(self.root), [])
        self.assertTrue((self.root / ".changeset/README.md").exists())

    def test_multiple_patches_bump_once(self):
        self.add_changeset("one", "patch", "First fix.")
        self.add_changeset("two", "patch", "Second fix.")
        self.assertEqual(release.prepare(self.root), "0.1.1")

    def test_major_resets_lower_components(self):
        self.add_changeset("breaking", "major", "Breaking change.")
        self.assertEqual(release.prepare(self.root), "1.0.0")

    def test_empty_release_fails_without_writing(self):
        with self.assertRaisesRegex(ValueError, "No changesets"):
            release.prepare(self.root)
        self.assertEqual(release.version(self.root), "0.1.0")
        self.assertFalse((self.root / "CHANGELOG.md").exists())

    def test_invalid_changeset_does_not_consume_valid_files(self):
        valid = self.add_changeset("valid", "patch", "A fix.")
        invalid = self.add_changeset("invalid", "typo", "Bad bump.")
        with self.assertRaisesRegex(ValueError, "Invalid changeset"):
            release.prepare(self.root)
        self.assertTrue(valid.exists())
        self.assertTrue(invalid.exists())
        self.assertEqual(release.version(self.root), "0.1.0")

    def test_empty_summary_and_unknown_package_are_rejected(self):
        for text in ('', '---\nworkgrove: patch\n---\n\n', '---\nother: patch\n---\nA fix.\n'):
            with self.subTest(text=text):
                (self.root / ".changeset/invalid.md").write_text(text)
                with self.assertRaises(ValueError):
                    release.changesets(self.root)

    def test_empty_changeset_satisfies_pr_check(self):
        base = self.init_base()
        (self.root / ".changeset/ci.md").write_text("---\n---\n")
        self.commit()
        release.check_pr(self.root, base)

    def test_only_empty_changesets_do_not_prepare_a_release(self):
        path = self.root / ".changeset/ci.md"
        path.write_text("---\n\n---\n\nCI only.\n")
        original = {file: file.read_bytes() for file in self.root.rglob("*") if file.is_file()}
        with self.assertRaisesRegex(ValueError, "No changesets requesting a release"):
            release.prepare(self.root)
        self.assertEqual(original, {file: file.read_bytes() for file in self.root.rglob("*") if file.is_file()})

    def test_release_consumes_empty_changesets_without_changelog_entries(self):
        empty = self.root / ".changeset/ci.md"
        empty.write_text("---\n---\n\nCI only.\n")
        self.add_changeset("fix", "patch", "A CLI fix.")
        base = self.init_base()
        self.assertEqual(release.prepare(self.root), "0.1.1")
        self.assertFalse(empty.exists())
        self.assertIn("A CLI fix.", release.notes(self.root))
        self.assertNotIn("CI only.", release.notes(self.root))
        self.commit()
        release.check_pr(self.root, base)

    def test_existing_empty_changeset_does_not_satisfy_another_pr(self):
        (self.root / ".changeset/ci.md").write_text("---\n---\n")
        base = self.init_base()
        (self.root / "docs.md").write_text("Documentation")
        self.commit()
        with self.assertRaisesRegex(ValueError, "PR must add"):
            release.check_pr(self.root, base)

    def test_lock_mismatch_fails_before_any_writes(self):
        path = self.root / "Cargo.lock"
        path.write_text(path.read_text().replace('version = "0.1.0"', 'version = "0.0.1"'))
        pending = self.add_changeset("fix", "patch", "Fix.")
        with self.assertRaisesRegex(ValueError, "versions disagree"):
            release.prepare(self.root)
        self.assertTrue(pending.exists())
        self.assertEqual(release.version(self.root), "0.1.0")
        self.assertFalse((self.root / "CHANGELOG.md").exists())

    def test_release_notes_must_match_current_version(self):
        (self.root / "CHANGELOG.md").write_text("# Changelog\n\n## 0.0.1\n\nOld notes.\n")
        with self.assertRaisesRegex(ValueError, "No changelog entry"):
            release.notes(self.root)

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True).strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-m", "Test change")

    def init_base(self):
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        self.commit()
        return self.git("rev-parse", "HEAD")

    def test_pr_requires_its_own_new_changeset(self):
        self.add_changeset("existing", "patch", "An earlier change.")
        base = self.init_base()
        (self.root / "feature.txt").write_text("A new feature")
        self.commit()
        with self.assertRaisesRegex(ValueError, "PR must add"):
            release.check_pr(self.root, base)
        self.add_changeset("new", "minor", "A new feature.")
        self.commit()
        release.check_pr(self.root, base)

    def test_pr_without_any_changeset_fails(self):
        base = self.init_base()
        (self.root / "docs.md").write_text("Documentation")
        self.commit()
        with self.assertRaisesRegex(ValueError, "PR must add"):
            release.check_pr(self.root, base)

    def test_exact_generated_release_is_allowed_but_extra_changes_fail(self):
        self.add_changeset("existing", "patch", "A fix.")
        base = self.init_base()
        release.prepare(self.root)
        self.commit()
        release.check_pr(self.root, base)
        (self.root / "code.rs").write_text("Unexpected code")
        self.commit()
        with self.assertRaisesRegex(ValueError, "PR must add"):
            release.check_pr(self.root, base)

    def test_tampered_release_notes_fail(self):
        self.add_changeset("existing", "patch", "A fix.")
        base = self.init_base()
        release.prepare(self.root)
        changelog = self.root / "CHANGELOG.md"
        changelog.write_text(changelog.read_text().replace("A fix.", "Different notes."))
        self.commit()
        with self.assertRaisesRegex(ValueError, "PR must add"):
            release.check_pr(self.root, base)


if __name__ == "__main__":
    unittest.main()
