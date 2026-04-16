use crate::error::{AppError, AppResult};
use crate::model::{GenerationOptions, TreeNode};
use std::fs;
use std::path::{Path, PathBuf};

pub fn build_repo_tree(
    repo_path: &Path,
    output_dir: &Path,
    options: &GenerationOptions,
) -> AppResult<TreeNode> {
    let excluded_output = output_dir
        .strip_prefix(repo_path)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(PathBuf::from);

    build_tree_node(repo_path, repo_path, excluded_output.as_deref(), options)
}

fn build_tree_node(
    repo_root: &Path,
    current_path: &Path,
    excluded_output: Option<&Path>,
    options: &GenerationOptions,
) -> AppResult<TreeNode> {
    let metadata = fs::symlink_metadata(current_path).map_err(|error| {
        AppError::io(format!("failed to read {}", current_path.display()), error)
    })?;
    let relative_path = current_path.strip_prefix(repo_root).map_err(|error| {
        AppError::message(format!("failed to derive repo-relative path: {error}"))
    })?;
    let path = path_to_string(relative_path);
    let is_dir = metadata.file_type().is_dir();
    let name = if path.is_empty() {
        repo_display_name(repo_root)
    } else {
        match current_path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => {
                eprintln!(
                    "warning: skipped non-UTF-8 path under {}",
                    current_path.display()
                );
                return Ok(TreeNode {
                    path,
                    name: String::from("<skipped>"),
                    is_dir,
                    children: Vec::new(),
                });
            }
        }
    };

    let mut children = Vec::new();
    if is_dir {
        let entries = match fs::read_dir(current_path) {
            Ok(entries) => entries,
            Err(error) => {
                if current_path == repo_root {
                    return Err(AppError::io(
                        format!("failed to read directory {}", current_path.display()),
                        error,
                    ));
                }
                eprintln!(
                    "warning: skipped unreadable directory {}: {}",
                    current_path.display(),
                    error
                );
                return Ok(TreeNode {
                    path,
                    name,
                    is_dir,
                    children,
                });
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!(
                        "warning: skipped unreadable entry under {}: {}",
                        current_path.display(),
                        error
                    );
                    continue;
                }
            };

            let child_path = entry.path();
            let child_relative = match child_path.strip_prefix(repo_root) {
                Ok(relative) => relative,
                Err(error) => {
                    eprintln!(
                        "warning: skipped entry with invalid repo-relative path {}: {}",
                        child_path.display(),
                        error
                    );
                    continue;
                }
            };

            if should_skip_path(child_relative, excluded_output) {
                continue;
            }

            let child_name_utf8 = child_path.file_name().and_then(|name| name.to_str());
            if child_name_utf8.is_none() {
                eprintln!(
                    "warning: skipped non-UTF-8 path under {}",
                    child_path.display()
                );
                continue;
            }

            let child = match build_tree_node(repo_root, &child_path, excluded_output, options) {
                Ok(child) => child,
                Err(error) => {
                    eprintln!("warning: skipped path {}: {}", child_path.display(), error);
                    continue;
                }
            };

            if child.is_dir
                && !options
                    .matcher
                    .keep_dir(&child.path, !child.children.is_empty())
            {
                continue;
            }
            if !child.is_dir && !options.matcher.matches_file(&child.path) {
                continue;
            }

            children.push(child);
        }

        children.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
    }

    Ok(TreeNode {
        path,
        name,
        is_dir,
        children,
    })
}

fn should_skip_path(relative_path: &Path, excluded_output: Option<&Path>) -> bool {
    if relative_path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        == Some(".git")
    {
        return true;
    }

    excluded_output
        .map(|excluded| relative_path.starts_with(excluded))
        .unwrap_or(false)
}

pub fn specific_directory_chain(path: &str, is_dir: bool) -> Vec<String> {
    let components: Vec<_> = Path::new(path)
        .iter()
        .map(|component| component.to_string_lossy().to_string())
        .collect();
    let directory_count = if is_dir {
        components.len()
    } else {
        components.len().saturating_sub(1)
    };

    let mut directories = Vec::new();
    let mut current = PathBuf::new();
    for component in components.iter().take(directory_count) {
        current.push(component);
        directories.push(path_to_string(&current));
    }
    directories.reverse();
    directories
}

pub fn repo_display_name(repo_path: &Path) -> String {
    let direct_name = repo_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| *name != "." && *name != "..")
        .map(str::to_string);

    if let Some(name) = direct_name {
        return name;
    }

    fs::canonicalize(repo_path)
        .ok()
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| repo_path.display().to_string())
}

pub fn display_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
