use serde_json::Value;
use std::{fs, path::Path, process::Command};

struct Fixture {
    root: tempfile::TempDir,
    source: std::path::PathBuf,
    state: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        let repo = source.join("api");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test"]);
        fs::write(repo.join("file"), "initial\n").unwrap();
        git(&repo, &["add", "file"]);
        git(&repo, &["commit", "-m", "initial"]);
        let state = root.path().join("state");
        fs::create_dir(&state).unwrap();
        Self {
            root,
            source,
            state,
        }
    }
    fn cli(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_workgrove"))
            .env("WORKGROVE_STATE_DIR", &self.state)
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .args(args)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into(),
            String::from_utf8_lossy(&out.stderr).into(),
        )
    }
    fn cli_with_path(&self, args: &[&str], path: &Path) -> (i32, String, String) {
        let system_path = std::env::var_os("PATH").unwrap_or_default();
        let out = Command::new(env!("CARGO_BIN_EXE_workgrove"))
            .env("WORKGROVE_STATE_DIR", &self.state)
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env(
                "PATH",
                format!("{}:{}", path.display(), system_path.to_string_lossy()),
            )
            .args(args)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into(),
            String::from_utf8_lossy(&out.stderr).into(),
        )
    }
    fn create(&self) -> Value {
        let (_, out, err) = self.cli(&[
            "--json",
            "create",
            "task",
            "--source",
            self.source.to_str().unwrap(),
            "--destination",
            self.root.path().join("workspace").to_str().unwrap(),
        ]);
        assert!(err.is_empty(), "{err}");
        serde_json::from_str(&out).unwrap()
    }
}
fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed")
}

#[test]
fn create_status_dirty_block_and_remove() {
    let f = Fixture::new();
    let record = f.create();
    assert_eq!(record["state"], "ready");
    assert_eq!(record["repos"].as_array().unwrap().len(), 1);
    let (_, status, _) = f.cli(&["--json", "status", "task"]);
    assert_eq!(
        serde_json::from_str::<Value>(&status).unwrap()["repos"][0]["blockers"],
        serde_json::json!([])
    );
    fs::write(f.root.path().join("workspace/api/valuable"), "keep").unwrap();
    let (code, blocked, _) = f.cli(&["--json", "remove", "task"]);
    assert_eq!(code, 2);
    assert_eq!(
        serde_json::from_str::<Value>(&blocked).unwrap()["safe_to_remove"],
        false
    );
    assert!(f.root.path().join("workspace/api").exists());
    fs::remove_file(f.root.path().join("workspace/api/valuable")).unwrap();
    let (code, removed, err) = f.cli(&["--json", "remove", "task"]);
    assert_eq!(code, 0, "{err}: {removed}");
    assert_eq!(
        serde_json::from_str::<Value>(&removed).unwrap()["state"],
        "removed"
    );
    assert!(!f.root.path().join("workspace").exists());
}

#[test]
fn ignored_files_and_workspace_root_files_block_cleanup() {
    let f = Fixture::new();
    f.create();
    let worktree = f.root.path().join("workspace/api");
    let exclude = git_output(&worktree, &["rev-parse", "--git-path", "info/exclude"]);
    fs::write(&exclude, "\nsecret.env\n").unwrap();
    fs::write(worktree.join("secret.env"), "keep").unwrap();
    let (code, report, _) = f.cli(&["--json", "remove", "task"]);
    assert_eq!(code, 2);
    assert!(report.contains("ignored files"));
    fs::remove_file(worktree.join("secret.env")).unwrap();
    fs::write(f.root.path().join("workspace/notes.md"), "keep").unwrap();
    let (code, report, _) = f.cli(&["--json", "remove", "task"]);
    assert_eq!(code, 2);
    assert!(report.contains("Files outside managed repositories"));
}

#[test]
fn recovery_resumes_partially_created_group() {
    let f = Fixture::new();
    let dest = f.root.path().join("workspace");
    let state = Command::new(env!("CARGO_BIN_EXE_workgrove"))
        .env("WORKGROVE_STATE_DIR", &f.state)
        .args([
            "create",
            "task",
            "--source",
            f.source.to_str().unwrap(),
            "--destination",
            dest.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(state.success());
    let record_path = f.state.join("task.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    record["state"] = serde_json::json!("removing");
    record["repos"][0]["state"] = serde_json::json!("removing");
    fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    git(
        &f.source.join("api"),
        &["worktree", "remove", dest.join("api").to_str().unwrap()],
    );
    let (code, out, err) = f.cli(&["resume", "task"]);
    assert_eq!(code, 0, "{err}: {out}");
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap()["state"],
        "removed"
    );
}

#[test]
fn base_override_and_unmerged_commit_block() {
    let f = Fixture::new();
    let (_, out, err) = f.cli(&[
        "create",
        "task",
        "--source",
        f.source.to_str().unwrap(),
        "--destination",
        f.root.path().join("workspace").to_str().unwrap(),
        "--base",
        "api=main",
    ]);
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("ready"));
    let repo = f.root.path().join("workspace/api");
    fs::write(repo.join("file"), "change\n").unwrap();
    git(&repo, &["commit", "-am", "feature"]);
    let (code, report, _) = f.cli(&["--json", "check", "task"]);
    assert_eq!(code, 2);
    assert!(report.contains("Commits are not merged"));
}

#[test]
fn merged_github_pr_with_exact_head_allows_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let source_repo = f.source.join("api");
    let bare = f.root.path().join("remote.git");
    let status = Command::new("git")
        .args(["init", "--bare", bare.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    git(
        &source_repo,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    git(&source_repo, &["push", "-u", "origin", "main"]);
    git(&source_repo, &["remote", "set-head", "origin", "main"]);
    git(
        &source_repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/example/api.git",
        ],
    );
    let dest = f.root.path().join("workspace");
    let bin = f.root.path().join("fake-bin");
    fs::create_dir(&bin).unwrap();
    let git_wrapper = bin.join("git");
    let real_git = Command::new("which").arg("git").output().unwrap();
    let real_git = String::from_utf8(real_git.stdout)
        .unwrap()
        .trim()
        .to_string();
    let wrapper = format!(
        "#!/bin/sh\nif [ \"$1\" = \"-C\" ] && [ \"$3\" = \"fetch\" ]; then exec \"{real_git}\" -C \"$2\" fetch --prune \"{}\"; fi\nexec \"{real_git}\" \"$@\"\n",
        bare.display()
    );
    fs::write(&git_wrapper, wrapper).unwrap();
    let mut perms = fs::metadata(&git_wrapper).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&git_wrapper, perms).unwrap();
    let (code, _, err) = f.cli_with_path(
        &[
            "create",
            "task",
            "--source",
            f.source.to_str().unwrap(),
            "--destination",
            dest.to_str().unwrap(),
        ],
        &bin,
    );
    assert_eq!(code, 0, "{err}");
    let worktree = dest.join("api");
    fs::write(worktree.join("file"), "feature\n").unwrap();
    git(&worktree, &["commit", "-am", "feature"]);
    let head = git_output(&worktree, &["rev-parse", "HEAD"]);
    let gh = bin.join("gh");
    let payload = format!(
        r#"[[{{"state":"closed","merged_at":"2026-01-01T00:00:00Z","html_url":"https://github.com/example/api/pull/1","head":{{"sha":"{head}"}}}}]]"#
    );
    let open_payload = format!(
        r#"[[{{"state":"open","merged_at":null,"html_url":"https://github.com/example/api/pull/1","head":{{"sha":"{head}"}}}}]]"#
    );
    fs::write(&gh, format!("#!/bin/sh\nprintf '%s' '{open_payload}'\n")).unwrap();
    let mut perms = fs::metadata(&gh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).unwrap();
    let (code, report, err) = f.cli_with_path(&["--json", "check", "task"], &bin);
    assert_eq!(code, 2, "{err}: {report}");
    assert!(report.contains("Open PR:"));
    fs::write(&gh, format!("#!/bin/sh\nprintf '%s' '{payload}'\n")).unwrap();
    let (code, report, err) = f.cli_with_path(&["--json", "check", "task"], &bin);
    assert_eq!(code, 0, "{err}: {report}");
    assert_eq!(
        serde_json::from_str::<Value>(&report).unwrap()["safe_to_remove"],
        true
    );
    let (code, report, err) = f.cli_with_path(&["--json", "remove", "task"], &bin);
    assert_eq!(code, 0, "{err}: {report}");
    assert_eq!(
        serde_json::from_str::<Value>(&report).unwrap()["state"],
        "removed"
    );
}
fn git_output(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn write_json(path: &Path, value: Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
fn directory_profiles_inherit_and_specific_settings_replace_arrays() {
    let f = Fixture::new();
    let web = f.source.join("web");
    fs::create_dir(&web).unwrap();
    git(&web, &["init", "-b", "main"]);
    // Exclusion must happen before Git tries to resolve this empty repo's HEAD.
    write_json(
        &f.root.path().join("config/workgrove/config.json"),
        serde_json::json!({
            f.source.to_str().unwrap(): {
                "workspace_destination": "../trees",
                "include": ["./api", "./web"],
                "after_create": ["exit 31"],
                "after_remove": ["exit 32"],
                "directories": { "./api": { "after_create": ["exit 33"] } }
            },
            "/unrelated/workgrove-test-root": {"after_create": ["exit 34"]}
        }),
    );
    write_json(
        &f.root.path().join(".workgrove.json"),
        serde_json::json!({
            "./source": {"exclude": ["./web"], "after_remove": []}
        }),
    );
    write_json(
        &f.source.join("api/.workgrove.json"),
        serde_json::json!({
            "after_create": ["printf local > hook-result"]
        }),
    );
    let (code, out, err) = f.cli(&["create", "task", "--source", f.source.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}: {err}");
    let record: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(record["repos"].as_array().unwrap().len(), 1);
    assert_eq!(
        record["repos"][0]["after_create"],
        serde_json::json!(["printf local > hook-result"])
    );
    assert_eq!(record["repos"][0]["after_remove"], serde_json::json!([]));
    assert_eq!(
        fs::read_to_string(f.root.path().join("trees/task/api/hook-result")).unwrap(),
        "local"
    );
}

#[test]
fn current_repo_uses_flat_settings_and_relative_destination() {
    let f = Fixture::new();
    let repo = f.source.join("api");
    write_json(
        &repo.join(".workgrove.json"),
        serde_json::json!({
            "workspace_destination": "./.workgrove/trees", "after_create": []
        }),
    );
    let out = Command::new(env!("CARGO_BIN_EXE_workgrove"))
        .current_dir(&repo)
        .env("WORKGROVE_STATE_DIR", &f.state)
        .env("XDG_CONFIG_HOME", f.root.path().join("config"))
        .args(["create", "task"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        record["path"],
        serde_json::json!(repo.canonicalize().unwrap().join(".workgrove/trees/task"))
    );
    assert!(repo.join(".workgrove/trees/task/api/file").is_file());
}

#[test]
fn missing_settings_use_default_destination() {
    let f = Fixture::new();
    let (code, out, err) = f.cli(&["create", "task", "--source", f.source.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}: {err}");
    assert!(f.source.join(".workgrove/trees/task/api/file").is_file());
}

#[test]
fn explicit_repository_selection_overrides_config_filters() {
    let f = Fixture::new();
    write_json(
        &f.source.join(".workgrove.json"),
        serde_json::json!({
            "include": [], "exclude": ["./api"], "workspace_destination": "./unused"
        }),
    );
    let destination = f.root.path().join("explicit");
    let (code, out, err) = f.cli(&[
        "create",
        "task",
        "--source",
        f.source.to_str().unwrap(),
        "--repo",
        "api",
        "--destination",
        destination.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}: {err}");
    assert!(destination.join("api/file").is_file());
    assert!(!f.source.join("unused").exists());
}

#[test]
fn root_profiles_run_distinct_hooks_for_each_included_repo() {
    let f = Fixture::new();
    let web = f.source.join("web");
    fs::create_dir(&web).unwrap();
    git(&web, &["init", "-b", "main"]);
    git(&web, &["config", "user.email", "test@example.com"]);
    git(&web, &["config", "user.name", "Test"]);
    git(&web, &["commit", "--allow-empty", "-m", "initial"]);
    write_json(
        &f.source.join(".workgrove.json"),
        serde_json::json!({
            ".": {
                "include": ["./api", "./web"],
                "after_create": ["exit 35"],
                "directories": {
                    "./api": {"after_create": ["printf api > marker"]},
                    "./web": {"after_create": ["printf web > marker"]}
                }
            }
        }),
    );
    let (code, out, err) = f.cli(&["create", "task", "--source", f.source.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}: {err}");
    for repo in ["api", "web"] {
        assert_eq!(
            fs::read_to_string(
                f.source
                    .join(format!(".workgrove/trees/task/{repo}/marker"))
            )
            .unwrap(),
            repo
        );
    }
}
