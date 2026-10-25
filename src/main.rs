use clap::{Parser, Subcommand};
use fs2::FileExt;
mod settings;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
#[command(
    name = "workgrove",
    about = "Create and retire groups of independent Git worktrees"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    Create {
        name: String,
        #[arg(long, default_value = ".")]
        source: PathBuf,
        #[arg(long)]
        destination: Option<PathBuf>,
        #[arg(long = "repo")]
        repos: Vec<String>,
        #[arg(long = "base")]
        bases: Vec<String>,
    },
    Resume {
        name: String,
    },
    Status {
        name: String,
    },
    Check {
        name: String,
    },
    Remove {
        name: String,
    },
    List,
}
type R<T> = Result<T, String>;
fn run_hooks(commands: &[String], cwd: &Path, name: &str, workspace: &Path, repo: &str) -> R<()> {
    for command in commands {
        let expanded = command
            .replace("{name}", name)
            .replace("{workspace}", &workspace.to_string_lossy())
            .replace("{repo}", repo);
        let output = Command::new("sh")
            .args(["-c", &expanded])
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("Cannot start lifecycle command `{command}`: {e}"))?;
        if !output.stdout.is_empty() {
            std::io::stderr()
                .write_all(&output.stdout)
                .map_err(|e| e.to_string())?;
        }
        if !output.stderr.is_empty() {
            std::io::stderr()
                .write_all(&output.stderr)
                .map_err(|e| e.to_string())?;
        }
        if !output.status.success() {
            return Err(format!(
                "Lifecycle command failed ({}) in {}: {command}",
                output.status,
                cwd.display()
            ));
        }
    }
    Ok(())
}
fn run(cmd: &str, args: &[&str], cwd: Option<&Path>, allowed: &[i32]) -> R<Output> {
    let mut c = Command::new(cmd);
    c.args(args);
    if let Some(p) = cwd {
        c.current_dir(p);
    }
    let o = c.output().map_err(|e| e.to_string())?;
    if !allowed.contains(&o.status.code().unwrap_or(-1)) {
        return Err(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(o)
}
fn git(p: &Path, args: &[&str]) -> R<String> {
    Ok(String::from_utf8_lossy(
        &run(
            "git",
            &[&["-C", p.to_str().unwrap_or("")][..], args].concat(),
            None,
            &[0],
        )?
        .stdout,
    )
    .trim()
    .to_string())
}
fn valid(s: &str) -> R<()> {
    if !s.is_empty()
        && s.len() <= 80
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        Ok(())
    } else {
        Err("Names must be 1–80 letters, digits, dots, underscores or hyphens, starting with a letter or digit".into())
    }
}
fn state() -> PathBuf {
    std::env::var_os("WORKGROVE_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".local/state/workgrove")))
        .unwrap_or(PathBuf::from(".local/state/workgrove"))
}
fn record_path(n: &str) -> R<PathBuf> {
    valid(n)?;
    Ok(state().join(format!("{n}.json")))
}
fn resolve(path: &Path) -> R<PathBuf> {
    if path.exists() {
        return path.canonicalize().map_err(|e| e.to_string());
    }
    let parent = path.parent().ok_or("Destination has no parent")?;
    let name = path
        .file_name()
        .ok_or("Destination has no final component")?;
    Ok(resolve(parent)?.join(name))
}
fn expand_user(path: &Path) -> PathBuf {
    if path == Path::new("~") {
        return dirs::home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}
fn save(v: &Value) -> R<()> {
    let p = record_path(v["name"].as_str().ok_or("Invalid record")?)?;
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut f, v).map_err(|e| e.to_string())?;
    f.write_all(b"\n").map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    fs::rename(tmp, p).map_err(|e| e.to_string())
}
fn load(n: &str) -> R<Value> {
    let v: Value = serde_json::from_slice(
        &fs::read(record_path(n)?).map_err(|e| format!("Cannot load workspace {n}: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    if v["version"] != 1 || v["name"] != n {
        return Err("Unsupported or invalid workspace record".into());
    }
    Ok(v)
}
fn lock() -> R<File> {
    let p = state();
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    let f = File::options()
        .create(true)
        .append(true)
        .open(p.join(".lock"))
        .map_err(|e| e.to_string())?;
    f.try_lock_exclusive()
        .map_err(|_| "Another workgrove command is running".to_string())?;
    Ok(f)
}
fn create(
    name: &str,
    source: &Path,
    dest: Option<&Path>,
    selected: &[String],
    bases: &[String],
) -> R<Value> {
    valid(name)?;
    if record_path(name)?.exists() {
        return Err("Workspace name already exists; use resume or choose a new name".into());
    }
    let src = expand_user(source)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let workspace_settings = settings::load(&src)?;
    let destination = match dest {
        Some(path) => {
            settings::absolute(path, &std::env::current_dir().map_err(|e| e.to_string())?)?
        }
        None => workspace_settings
            .workspace_destination
            .clone()
            .unwrap_or_else(|| src.join(".workgrove/trees"))
            .join(name),
    };
    if destination.exists() {
        return Err("Destination must not exist".into());
    }
    let dest = resolve(&expand_user(&destination))?;
    if resolve(&state())?.starts_with(&dest) {
        return Err("Recovery records must be outside the workspace".into());
    }
    let mut base_overrides = BTreeMap::new();
    for spec in bases {
        let (repo, base) = spec.split_once('=').ok_or("Use --base repository=ref")?;
        valid(repo)?;
        if base.is_empty() || base.starts_with('-') {
            return Err("Invalid base ref".into());
        }
        base_overrides.insert(repo, base);
    }
    let mut paths = if !selected.is_empty() {
        selected
            .iter()
            .map(|n| {
                valid(n)?;
                Ok(src.join(n))
            })
            .collect::<R<Vec<_>>>()?
    } else if src.join(".git").exists() {
        vec![src.clone()]
    } else if let Some(include) = &workspace_settings.include {
        include.clone()
    } else {
        let mut discovered = Vec::new();
        for entry in fs::read_dir(&src).map_err(|e| e.to_string())? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_dir() && !p.is_symlink() && p.join(".git").exists() {
                discovered.push(p);
            }
        }
        discovered.sort();
        discovered
    };
    if selected.is_empty() {
        paths.retain(|p| !workspace_settings.exclude.contains(p));
    }
    let mut unique = BTreeSet::new();
    paths.retain(|p| unique.insert(p.clone()));
    for p in &paths {
        if (p != &src && p.parent() != Some(src.as_path()))
            || !p.is_dir()
            || p.is_symlink()
            || !p.join(".git").exists()
        {
            return Err(format!(
                "Not a direct, non-symlink Git checkout: {}",
                p.display()
            ));
        }
    }
    if paths.is_empty() {
        return Err(
            "No repositories found; expected immediate child directories containing .git".into(),
        );
    }
    let repo_names: BTreeSet<String> = paths
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    if let Some(unknown) = base_overrides
        .keys()
        .find(|repo| !repo_names.contains(**repo))
    {
        return Err(format!(
            "--base names a repository that is not selected: {unknown}"
        ));
    }
    let mut entries = vec![];
    let mut identities = BTreeSet::new();
    for p in paths {
        let root = PathBuf::from(git(&p, &["rev-parse", "--show-toplevel"])?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if root != p.canonicalize().map_err(|e| e.to_string())? {
            return Err(format!("Not a repository root: {}", p.display()));
        }
        if p.join(".gitmodules").exists() {
            return Err(format!(
                "Submodule repositories are not supported: {}",
                p.display()
            ));
        }
        let nm = p.file_name().unwrap().to_string_lossy().to_string();
        let repo_settings = settings::load(&p)?;
        if dest.starts_with(&p) && p != src {
            return Err(
                "Destination must be outside source repositories in a multi-repo workspace".into(),
            );
        }
        let rem = git(&p, &["remote"])?;
        let remotes: Vec<_> = rem.lines().collect();
        if !remotes.is_empty() && remotes != ["origin"] {
            return Err(format!(
                "{nm}: first version supports exactly one origin remote, or no remotes"
            ));
        }
        let remote = if remotes.is_empty() {
            Value::Null
        } else {
            json!(git(&p, &["remote", "get-url", "origin"])?)
        };
        if !remote.is_null() {
            git(&p, &["fetch", "--prune", "origin"])?;
        }
        let base = base_overrides
            .get(nm.as_str())
            .map(|base| base.to_string())
            .unwrap_or(if remote.is_null() {
                git(&p, &["symbolic-ref", "HEAD"])?
            } else {
                git(&p, &["symbolic-ref", "refs/remotes/origin/HEAD"])?
            });
        let initial = git(
            &p,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )?;
        let branch = format!("workgrove/{name}");
        if run(
            "git",
            &[
                "-C",
                p.to_str().unwrap(),
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
            None,
            &[0, 1],
        )?
        .status
        .success()
        {
            return Err(format!("{nm}: branch {branch} already exists"));
        }
        let common = git(
            &p,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        if !identities.insert(common.clone()) {
            return Err("Source includes multiple checkouts of the same repository".into());
        }
        let after_create = repo_settings.after_create;
        let after_remove = repo_settings.after_remove;
        entries.push(json!({"name":nm,"source":p,"path":dest.join(&nm),"common_dir":common,"base":base,"initial":initial,"branch":branch,"remote":remote,"state":"planned","after_create":after_create,"after_remove":after_remove,"create_hooks_done":false,"remove_hooks_done":false}));
    }
    let v = json!({"version":1,"name":name,"path":dest,"source":src,"state":"creating","created_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64(),"repos":entries});
    save(&v)?;
    finish_create(v)
}
fn finish_create(mut r: Value) -> R<Value> {
    if r["state"] != "creating" && r["state"] != "ready" {
        return Err("Cannot resume creation after removal has started".into());
    }
    let root = PathBuf::from(r["path"].as_str().unwrap());
    if root.is_symlink() {
        return Err("Workspace root became a symlink".into());
    }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let len = r["repos"].as_array().unwrap().len();
    for i in 0..len {
        let e = r["repos"][i].clone();
        let p = PathBuf::from(e["path"].as_str().unwrap());
        if p.exists() {
            verify(&e)?
        } else {
            let b = e["branch"].as_str().unwrap();
            let src = PathBuf::from(e["source"].as_str().unwrap());
            let exists = run(
                "git",
                &[
                    "-C",
                    src.to_str().unwrap(),
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{b}"),
                ],
                None,
                &[0, 1],
            )?
            .status
            .success();
            if exists {
                if e["state"] != "creating" || git(&src, &["rev-parse", b])? != e["initial"] {
                    return Err("Unexpected existing branch; inspect the creation record".into());
                }
                git(&src, &["worktree", "add", p.to_str().unwrap(), b])?;
            } else {
                r["repos"][i]["state"] = json!("creating");
                save(&r)?;
                git(
                    &src,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        b,
                        p.to_str().unwrap(),
                        e["initial"].as_str().unwrap(),
                    ],
                )?;
            }
            verify(&e)?
        }
        r["repos"][i]["state"] = json!("ready");
        save(&r)?;
    }
    r["state"] = json!("ready");
    save(&r)?;
    for i in 0..len {
        if r["repos"][i]["create_hooks_done"] == true {
            continue;
        }
        let e = r["repos"][i].clone();
        let hooks: Vec<String> =
            serde_json::from_value(e["after_create"].clone()).unwrap_or_default();
        run_hooks(
            &hooks,
            Path::new(e["path"].as_str().unwrap()),
            r["name"].as_str().unwrap(),
            &root,
            e["name"].as_str().unwrap(),
        )?;
        r["repos"][i]["create_hooks_done"] = json!(true);
        save(&r)?;
    }
    Ok(r)
}
fn verify(e: &Value) -> R<()> {
    let p = PathBuf::from(e["path"].as_str().unwrap());
    if p.is_symlink()
        || !p.is_dir()
        || git(
            &p,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )? != e["common_dir"].as_str().unwrap()
    {
        return Err("Checkout identity changed".into());
    }
    let list = git(
        Path::new(e["source"].as_str().unwrap()),
        &["worktree", "list", "--porcelain"],
    )?;
    if !list.contains(&format!("worktree {}", p.display())) {
        return Err("Checkout is not registered with its source repository".into());
    }
    if git(&p, &["symbolic-ref", "--short", "HEAD"])? != e["branch"] {
        return Err("Checkout branch changed or HEAD is detached".into());
    }
    Ok(())
}
fn github_slug(remote: &str) -> R<String> {
    let rest = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .ok_or(
            "Remote provider/URL unsupported for PR verification; only github.com is supported",
        )?;
    let rest = rest
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(rest.trim_end_matches('/'));
    let parts: Vec<_> = rest.split('/').collect();
    if parts.len() != 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(
            "Remote provider/URL unsupported for PR verification; only github.com is supported"
                .into(),
        );
    }
    Ok(rest.to_string())
}
fn review_state(e: &Value, head: &str) -> R<(Vec<String>, bool)> {
    let slug = github_slug(e["remote"].as_str().ok_or("Missing origin URL")?)?;
    let owner = slug.split('/').next().unwrap();
    let endpoint = format!("repos/{slug}/pulls");
    let head_filter = format!("{}:{}", owner, e["branch"].as_str().unwrap());
    let out = run(
        "gh",
        &[
            "api",
            "--method",
            "GET",
            &endpoint,
            "--paginate",
            "--slurp",
            "-f",
            "state=all",
            "-f",
            &format!("head={head_filter}"),
            "-f",
            "per_page=100",
        ],
        None,
        &[0],
    )?;
    let pages: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Invalid GitHub PR response: {e}"))?;
    let mut blockers = vec![];
    let mut merged = false;
    for page in pages.as_array().ok_or("Invalid GitHub PR response")? {
        for pr in page.as_array().ok_or("Invalid GitHub PR response")? {
            let url = pr["html_url"]
                .as_str()
                .ok_or("Invalid GitHub PR response")?;
            if pr["state"] == "open" {
                blockers.push(format!("Open PR: {url}"))
            } else if pr["merged_at"].is_null() {
                blockers.push(format!("Closed, unmerged PR: {url}"))
            }
            if !pr["merged_at"].is_null() && pr["head"]["sha"].as_str() == Some(head) {
                merged = true
            }
        }
    }
    Ok((blockers, merged))
}
// Inspection and removal stay deliberately conservative: any dirty, unmerged or externally used checkout blocks cleanup.
fn inspect_repo(e: &Value, refresh: bool) -> Value {
    let name = e["name"].clone();
    let mut b: Vec<String> = vec![];
    let p = PathBuf::from(e["path"].as_str().unwrap());
    if e["state"] == "removed" {
        return json!({"name":name,"head":null,"blockers":b});
    }
    let result = (|| -> R<String> {
        verify(e)?;
        let head = git(&p, &["rev-parse", "HEAD"])?;
        if !git(
            &p,
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored=matching",
            ],
        )?
        .is_empty()
        {
            b.push("Modified, untracked or ignored files present".into())
        }
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "BISECT_START",
        ] {
            let path = git(&p, &["rev-parse", "--git-path", marker])?;
            if Path::new(&path).exists() {
                b.push(format!("Git operation in progress: {marker}"))
            }
        }
        let rem = git(&p, &["remote"])?;
        let expected = if e["remote"].is_null() { "" } else { "origin" };
        if rem != expected {
            return Err("Remote configuration changed; review manually before cleanup".into());
        }
        let mut merged_pr = false;
        if !e["remote"].is_null() {
            if git(&p, &["remote", "get-url", "origin"])? != e["remote"].as_str().unwrap() {
                return Err("Origin URL changed".into());
            }
            if refresh {
                git(&p, &["fetch", "--prune", "origin"])?;
                let (pr_blockers, merged) = review_state(e, &head)?;
                b.extend(pr_blockers);
                merged_pr = merged;
            } else {
                b.push("Remote/PR status not verified; run check".into())
            }
        }
        let base = git(
            &p,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", e["base"].as_str().unwrap()),
            ],
        )?;
        let ancestry = run(
            "git",
            &[
                "-C",
                p.to_str().unwrap(),
                "merge-base",
                "--is-ancestor",
                &head,
                &base,
            ],
            None,
            &[0, 1],
        )?
        .status
        .success();
        if !ancestry && !merged_pr {
            b.push(
                "Commits are not merged into the configured base (or a verified merged PR)".into(),
            )
        }
        if head != e["initial"] && !e["remote"].is_null() && !merged_pr {
            let refs = git(
                &p,
                &[
                    "for-each-ref",
                    "--contains",
                    &head,
                    "--format=%(refname)",
                    "refs/remotes/origin/",
                ],
            )?;
            if refs.is_empty() {
                b.push("Local commits have no verified copy on origin".into())
            }
        }
        Ok(head)
    })();
    match result {
        Ok(h) => json!({"name":name,"head":h,"blockers":b}),
        Err(x) => {
            b.push(x);
            json!({"name":name,"head":null,"blockers":b})
        }
    }
}
fn inspect(r: &Value, refresh: bool) -> Value {
    let root = PathBuf::from(r["path"].as_str().unwrap());
    let mut b = vec![];
    if root.is_symlink() {
        b.push("Workspace root became a symlink".to_string())
    } else if root.exists() {
        let expected: BTreeSet<String> = r["repos"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["state"] != "removed")
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect();
        if let Ok(rd) = fs::read_dir(&root) {
            let extras: Vec<_> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| !expected.contains(n))
                .collect();
            if !extras.is_empty() {
                b.push(format!(
                    "Files outside managed repositories: {}",
                    extras.join(", ")
                ))
            }
        }
        if refresh {
            b.extend(process_blockers(&root))
        }
    } else if r["repos"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["state"] != "removed")
    {
        b.push("Workspace directory is missing".into())
    }
    let repos: Vec<_> = r["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| inspect_repo(e, refresh))
        .collect();
    let safe = refresh
        && b.is_empty()
        && repos
            .iter()
            .all(|e| e["blockers"].as_array().unwrap().is_empty());
    json!({"name":r["name"],"path":r["path"],"state":r["state"],"safe_to_remove":safe,"blockers":b,"repos":repos})
}
fn process_blockers(root: &Path) -> Vec<String> {
    let o = match run(
        "lsof",
        &["-nP", "+D", root.to_str().unwrap_or(""), "-Fp"],
        None,
        &[0, 1],
    ) {
        Ok(o) => o,
        Err(_) => return vec!["lsof is required to check processes using the workspace".into()],
    };
    let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
    if !stderr.is_empty() {
        return vec![format!("Process inspection incomplete: {stderr}")];
    }
    let pids: BTreeSet<_> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| l.starts_with('p'))
        .map(|l| l[1..].to_string())
        .collect();
    if pids.is_empty() {
        vec![]
    } else {
        vec![format!(
            "Processes have open files or working directories in workspace: {}",
            pids.into_iter().collect::<Vec<_>>().join(", ")
        )]
    }
}
fn remove(mut r: Value) -> R<Value> {
    let report = inspect(&r, true);
    if report["safe_to_remove"] != true {
        return Ok(report);
    }
    r["state"] = json!("removing");
    save(&r)?;
    let len = r["repos"].as_array().unwrap().len();
    for i in 0..len {
        if r["repos"][i]["state"] == "removed" {
            continue;
        }
        let e = r["repos"][i].clone();
        let ck = inspect_repo(&e, true);
        let active = process_blockers(Path::new(r["path"].as_str().unwrap()));
        if !ck["blockers"].as_array().unwrap().is_empty() || !active.is_empty() {
            return Err(format!(
                "Cleanup stopped: {}; {}",
                ck["blockers"],
                active.join("; ")
            ));
        }
        r["repos"][i]["state"] = json!("removing");
        save(&r)?;
        git(
            Path::new(e["source"].as_str().unwrap()),
            &["worktree", "remove", e["path"].as_str().unwrap()],
        )?;
        r["repos"][i]["state"] = json!("removed");
        save(&r)?;
    }
    let root = PathBuf::from(r["path"].as_str().unwrap());
    if root.exists() {
        fs::remove_dir(&root).map_err(|e| e.to_string())?
    }
    r["state"] = json!("removed");
    save(&r)?;
    finish_remove_hooks(r)
}
fn finish_remove_hooks(mut r: Value) -> R<Value> {
    let len = r["repos"].as_array().ok_or("Invalid record")?.len();
    for i in 0..len {
        if r["repos"][i]["remove_hooks_done"] == true {
            continue;
        }
        let e = r["repos"][i].clone();
        let hooks: Vec<String> =
            serde_json::from_value(e["after_remove"].clone()).unwrap_or_default();
        run_hooks(
            &hooks,
            Path::new(e["source"].as_str().unwrap()),
            r["name"].as_str().unwrap(),
            Path::new(r["path"].as_str().unwrap()),
            e["name"].as_str().unwrap(),
        )?;
        r["repos"][i]["remove_hooks_done"] = json!(true);
        save(&r)?;
    }
    Ok(r)
}
fn reconcile_removal(r: &mut Value) -> R<()> {
    let len = r["repos"].as_array().ok_or("Invalid record")?.len();
    for i in 0..len {
        if r["repos"][i]["state"] == "removing" {
            let e = r["repos"][i].clone();
            if !Path::new(e["path"].as_str().unwrap()).exists()
                && !git(
                    Path::new(e["source"].as_str().unwrap()),
                    &["worktree", "list", "--porcelain"],
                )?
                .contains(&format!("worktree {}", e["path"].as_str().unwrap()))
            {
                r["repos"][i]["state"] = json!("removed");
                save(r)?
            }
        }
    }
    Ok(())
}
fn main() {
    let cli = Cli::parse();
    let _lock = match lock() {
        Ok(f) => f,
        Err(e) => {
            fail(e, cli.json);
            return;
        }
    };
    let result = (|| -> R<(Value, bool)> {
        match cli.command {
            Cmd::Create {
                name,
                source,
                destination,
                repos,
                bases,
            } => Ok((
                create(&name, &source, destination.as_deref(), &repos, &bases)?,
                false,
            )),
            Cmd::List => {
                let mut files: Vec<PathBuf> = fs::read_dir(state())
                    .map_err(|e| e.to_string())?
                    .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
                    .collect::<R<Vec<_>>>()?;
                files.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
                files.sort();
                let mut records = Vec::with_capacity(files.len());
                for path in files {
                    let bytes = fs::read(path).map_err(|e| e.to_string())?;
                    records
                        .push(serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())?);
                }
                Ok((json!(records), false))
            }
            Cmd::Resume { name } => {
                let mut r = load(&name)?;
                reconcile_removal(&mut r)?;
                if r["state"] == "removed" {
                    Ok((finish_remove_hooks(r)?, false))
                } else if r["state"] == "removing" {
                    Ok((remove(r)?, true))
                } else {
                    Ok((finish_create(r)?, false))
                }
            }
            Cmd::Status { name } => {
                let r = load(&name)?;
                Ok((inspect(&r, false), false))
            }
            Cmd::Check { name } => {
                let r = load(&name)?;
                Ok((inspect(&r, true), true))
            }
            Cmd::Remove { name } => {
                let mut r = load(&name)?;
                reconcile_removal(&mut r)?;
                if r["state"] == "removed" {
                    Ok((finish_remove_hooks(r)?, false))
                } else {
                    Ok((remove(r)?, true))
                }
            }
        }
    })();
    match result {
        Ok((v, code2)) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            if code2 && v["safe_to_remove"] == false {
                std::process::exit(2)
            }
        }
        Err(e) => fail(e, cli.json),
    }
}
fn fail(e: String, j: bool) {
    if j {
        eprintln!("{}", json!({"error":e}))
    } else {
        eprintln!("workgrove: {e}")
    }
    std::process::exit(1)
}
