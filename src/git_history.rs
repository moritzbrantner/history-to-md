use crate::error::{AppError, AppResult};
use crate::model::{CommitMeta, GenerationOptions, HistoryAccumulator, HistoryReport, PathHistory};
use crate::tree::path_to_string;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn collect_history(repo_path: &Path, options: &GenerationOptions) -> AppResult<HistoryReport> {
    ensure_git_repository(repo_path)?;

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo_path)
        .arg("log")
        .arg("--no-merges")
        .arg("--no-renames")
        .arg("--date=short")
        .arg("--pretty=format:__COMMIT__%n%H%x1f%ad%x1f%an%x1f%s")
        .arg("--numstat");

    if let Some(since) = options.since.as_deref() {
        command.arg(format!("--since={since}"));
    }
    if let Some(until) = options.until.as_deref() {
        command.arg(format!("--until={until}"));
    }
    if let Some(max_commits) = options.max_commits {
        command.arg(format!("-n{max_commits}"));
    }

    let output = command
        .output()
        .map_err(|error| AppError::io("failed to run git log", error))?;

    if !output.status.success() {
        return Err(AppError::message(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| AppError::utf8("git log output was not valid UTF-8", error))?;

    let mut report = HistoryReport::default();
    let mut files: HashMap<String, HistoryAccumulator> = HashMap::new();
    let mut directories: HashMap<String, HistoryAccumulator> = HashMap::new();
    directories.insert(String::new(), HistoryAccumulator::new(String::new()));

    let mut current_commit: Option<CommitMeta> = None;
    let mut expect_metadata = false;

    for line in stdout.lines() {
        if line == "__COMMIT__" {
            expect_metadata = true;
            current_commit = None;
            continue;
        }

        if expect_metadata {
            let commit = parse_commit_meta(line)?;
            report.scanned_commits += 1;
            current_commit = Some(commit);
            expect_metadata = false;
            continue;
        }

        if line.is_empty() {
            continue;
        }

        let Some(commit) = current_commit.as_ref() else {
            continue;
        };

        let Some((added, deleted, path)) = parse_numstat_line(line) else {
            continue;
        };

        if !options.matcher.matches_file(&path) {
            continue;
        }

        files
            .entry(path.clone())
            .or_insert_with(|| HistoryAccumulator::new(path.clone()))
            .record_change(commit, added, deleted);

        for directory in ancestor_directories(&path) {
            directories
                .entry(directory.clone())
                .or_insert_with(|| HistoryAccumulator::new(directory))
                .record_change(commit, added, deleted);
        }
    }

    report.file_histories = files
        .into_iter()
        .map(|(path, history)| (path, history.into_history()))
        .collect::<BTreeMap<_, _>>();
    report.directory_histories = directories
        .into_iter()
        .map(|(path, history)| (path, history.into_history()))
        .collect::<BTreeMap<_, _>>();

    Ok(report)
}

pub fn parse_commit_meta(line: &str) -> AppResult<CommitMeta> {
    let mut parts = line.split('\u{1f}');
    let hash = parts
        .next()
        .ok_or_else(|| AppError::message("missing commit hash in git log output"))?;
    let date = parts
        .next()
        .ok_or_else(|| AppError::message("missing commit date in git log output"))?;
    let author = parts
        .next()
        .ok_or_else(|| AppError::message("missing commit author in git log output"))?;
    let subject = parts
        .next()
        .ok_or_else(|| AppError::message("missing commit subject in git log output"))?;

    Ok(CommitMeta {
        hash: hash.to_string(),
        date: date.to_string(),
        author: author.to_string(),
        subject: subject.to_string(),
    })
}

pub fn parse_numstat_line(line: &str) -> Option<(u64, u64, String)> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?;
    let deleted = parts.next()?;
    let path = parts.next()?.trim();

    Some((
        parse_numstat_value(added),
        parse_numstat_value(deleted),
        path.to_string(),
    ))
}

fn parse_numstat_value(value: &str) -> u64 {
    value.parse::<u64>().unwrap_or(0)
}

pub fn ancestor_directories(file_path: &str) -> Vec<String> {
    let components: Vec<_> = Path::new(file_path)
        .iter()
        .map(|component| component.to_string_lossy().to_string())
        .collect();

    let mut directories = vec![String::new()];
    if components.len() <= 1 {
        return directories;
    }

    let mut current = PathBuf::new();
    for component in components.iter().take(components.len() - 1) {
        current.push(component);
        directories.push(path_to_string(&current));
    }

    directories
}

pub fn latest_activity_histories<'a>(
    histories: impl Iterator<Item = &'a PathHistory>,
) -> Vec<&'a PathHistory> {
    let mut items: Vec<_> = histories.collect();
    items.sort_by(|left, right| {
        let left_date = left
            .commits
            .first()
            .map(|commit| commit.commit.date.as_str());
        let right_date = right
            .commits
            .first()
            .map(|commit| commit.commit.date.as_str());
        right_date
            .cmp(&left_date)
            .then_with(|| right.commit_count.cmp(&left.commit_count))
            .then_with(|| left.path.cmp(&right.path))
    });
    items
}

fn ensure_git_repository(repo_path: &Path) -> AppResult<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|error| AppError::io("failed to check repository", error))?;

    if !output.status.success() {
        return Err(AppError::message(format!(
            "not a git repository: {}",
            repo_path.display()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != "true" {
        return Err(AppError::message(format!(
            "not a git repository: {}",
            repo_path.display()
        )));
    }

    Ok(())
}
