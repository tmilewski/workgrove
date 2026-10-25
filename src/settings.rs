use crate::{R, expand_user};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    workspace_destination: Option<PathBuf>,
    include: Option<Vec<PathBuf>>,
    exclude: Option<Vec<PathBuf>>,
    after_create: Option<Vec<String>>,
    after_remove: Option<Vec<String>>,
    #[serde(default)]
    directories: BTreeMap<String, Profile>,
}

#[derive(Default)]
pub struct Settings {
    pub workspace_destination: Option<PathBuf>,
    pub include: Option<Vec<PathBuf>>,
    pub exclude: Vec<PathBuf>,
    pub after_create: Vec<String>,
    pub after_remove: Vec<String>,
}

pub fn absolute(path: &Path, base: &Path) -> R<PathBuf> {
    let expanded = expand_user(path);
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        base.join(expanded)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.exists() {
        normalized.canonicalize().map_err(|e| e.to_string())
    } else {
        Ok(normalized)
    }
}

fn collect(
    profile: Profile,
    scope: PathBuf,
    target: &Path,
    layers: &mut Vec<(PathBuf, Profile)>,
) -> R<()> {
    if !target.starts_with(&scope) {
        return Ok(());
    }
    let mut profile = profile;
    let children = std::mem::take(&mut profile.directories);
    layers.push((scope.clone(), profile));
    for (path, child) in children {
        let child_scope = absolute(Path::new(&path), &scope)?;
        if !child_scope.starts_with(&scope) || child_scope == scope {
            return Err(format!(
                "Directory profile {path} must be below {}",
                scope.display()
            ));
        }
        collect(child, child_scope, target, layers)?;
    }
    Ok(())
}

pub fn load(target: &Path) -> R<Settings> {
    let global = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".config")))
        .ok_or("Cannot locate global configuration directory")?
        .join("workgrove/config.json");
    let mut files = vec![global];
    files.extend(
        target
            .ancestors()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|p| p.join(".workgrove.json")),
    );
    let mut layers = Vec::new();
    for (file_index, file) in files.into_iter().enumerate() {
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let result = (|| -> R<()> {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let object = value.as_object().ok_or("Settings must be an object")?;
            let flat = object.keys().any(|k| {
                matches!(
                    k.as_str(),
                    "workspace_destination"
                        | "include"
                        | "exclude"
                        | "after_create"
                        | "after_remove"
                        | "directories"
                )
            });
            if flat {
                if file_index == 0 {
                    return Err(
                        "Global settings must map directory paths to settings profiles".into(),
                    );
                }
                let profile: Profile = serde_json::from_value(value).map_err(|e| e.to_string())?;
                collect(
                    profile,
                    file.parent().unwrap().to_path_buf(),
                    target,
                    &mut layers,
                )?;
            } else {
                let profiles: BTreeMap<String, Profile> =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                for (path, profile) in profiles {
                    collect(
                        profile,
                        absolute(Path::new(&path), file.parent().unwrap())?,
                        target,
                        &mut layers,
                    )?;
                }
            }
            Ok(())
        })();
        result.map_err(|e| format!("Invalid settings at {}: {e}", file.display()))?;
    }
    // Stable ordering preserves file precedence when profiles have the same scope.
    layers.sort_by_key(|(scope, _)| scope.components().count());
    let mut effective = Settings::default();
    for (scope, profile) in layers {
        if let Some(path) = profile.workspace_destination {
            effective.workspace_destination = Some(absolute(&path, &scope)?);
        }
        if let Some(paths) = profile.include {
            effective.include = Some(
                paths
                    .iter()
                    .map(|p| absolute(p, &scope))
                    .collect::<R<_>>()?,
            );
        }
        if let Some(paths) = profile.exclude {
            effective.exclude = paths
                .iter()
                .map(|p| absolute(p, &scope))
                .collect::<R<_>>()?;
        }
        if let Some(commands) = profile.after_create {
            effective.after_create = commands;
        }
        if let Some(commands) = profile.after_remove {
            effective.after_remove = commands;
        }
    }
    Ok(effective)
}
