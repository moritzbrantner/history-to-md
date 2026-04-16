use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_OUTPUT_DIR: &str = "history-md";
const MARKDOWN_COMMITS_PER_NODE: usize = 12;
const SUMMARY_FILE_COUNT: usize = 20;
const SUMMARY_DIRECTORY_COUNT: usize = 15;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let config = Config::from_args(&args)?;
    let history = collect_history(&config.repo_path)?;
    let report = RepoReport {
        repo_name: repo_display_name(&config.repo_path),
        scanned_commits: history.scanned_commits,
        file_histories: history.file_histories,
        directory_histories: history.directory_histories,
        tree: build_repo_tree(&config.repo_path, &config.output_dir)?,
    };

    write_report(&config.output_dir, &report)?;

    println!(
        "Wrote {} file summaries, {} folder summaries, and {}",
        report.file_histories.len(),
        report.directory_histories.len().saturating_sub(1),
        config.output_dir.join("index.html").display()
    );

    Ok(())
}

struct Config {
    repo_path: PathBuf,
    output_dir: PathBuf,
}

impl Config {
    fn from_args(args: &[String]) -> Result<Self, String> {
        if args.len() < 2 || args.len() > 3 {
            return Err(format!(
                "usage: {} <repo-path> [output-dir]",
                args.first().map_or("history-to-md", String::as_str)
            ));
        }

        let repo_path = PathBuf::from(&args[1]);
        if !repo_path.exists() {
            return Err(format!(
                "repository path does not exist: {}",
                repo_path.display()
            ));
        }

        let output_dir = args
            .get(2)
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_path.join(DEFAULT_OUTPUT_DIR));

        Ok(Self {
            repo_path,
            output_dir,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
struct CommitMeta {
    hash: String,
    date: String,
    author: String,
    subject: String,
}

#[derive(Clone, Debug, Serialize)]
struct FileCommit {
    commit: CommitMeta,
    added: u64,
    deleted: u64,
}

#[derive(Debug, Default)]
struct PathHistory {
    path: String,
    commit_count: u64,
    total_added: u64,
    total_deleted: u64,
    authors: HashMap<String, u64>,
    commits: Vec<FileCommit>,
}

#[derive(Debug, Default)]
struct HistoryAccumulator {
    path: String,
    commit_count: u64,
    total_added: u64,
    total_deleted: u64,
    authors: HashMap<String, u64>,
    commits: Vec<FileCommit>,
    commit_indices: HashMap<String, usize>,
}

impl HistoryAccumulator {
    fn new(path: String) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }

    fn record_change(&mut self, commit: &CommitMeta, added: u64, deleted: u64) {
        self.total_added += added;
        self.total_deleted += deleted;

        if let Some(index) = self.commit_indices.get(&commit.hash).copied() {
            if let Some(existing) = self.commits.get_mut(index) {
                existing.added += added;
                existing.deleted += deleted;
            }
            return;
        }

        let index = self.commits.len();
        self.commit_indices.insert(commit.hash.clone(), index);
        self.commits.push(FileCommit {
            commit: commit.clone(),
            added,
            deleted,
        });
        self.commit_count += 1;
        *self.authors.entry(commit.author.clone()).or_insert(0) += 1;
    }

    fn into_history(self) -> PathHistory {
        PathHistory {
            path: self.path,
            commit_count: self.commit_count,
            total_added: self.total_added,
            total_deleted: self.total_deleted,
            authors: self.authors,
            commits: self.commits,
        }
    }
}

fn commit_preview(history: &PathHistory) -> impl Iterator<Item = &FileCommit> {
    history.commits.iter().take(MARKDOWN_COMMITS_PER_NODE)
}

#[derive(Debug, Default)]
struct HistoryReport {
    scanned_commits: u64,
    file_histories: HashMap<String, PathHistory>,
    directory_histories: HashMap<String, PathHistory>,
}

#[derive(Debug)]
struct RepoReport {
    repo_name: String,
    scanned_commits: u64,
    file_histories: HashMap<String, PathHistory>,
    directory_histories: HashMap<String, PathHistory>,
    tree: TreeNode,
}

#[derive(Clone, Debug)]
struct TreeNode {
    path: String,
    name: String,
    is_dir: bool,
    children: Vec<TreeNode>,
}

#[derive(Debug, Serialize)]
struct HtmlReportData {
    repo_name: String,
    scanned_commits: u64,
    changed_files: usize,
    changed_directories: usize,
    nodes: Vec<NodeView>,
}

#[derive(Debug, Serialize)]
struct NodeView {
    path: String,
    name: String,
    is_dir: bool,
    commit_count: u64,
    total_added: u64,
    total_deleted: u64,
    primary_authors: String,
    report_links: Vec<ReportLink>,
    commits: Vec<FileCommit>,
}

#[derive(Clone, Debug, Serialize)]
struct ReportLink {
    label: String,
    href: String,
}

fn collect_history(repo_path: &Path) -> Result<HistoryReport, String> {
    ensure_git_repository(repo_path)?;

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "log",
            "--no-merges",
            "--no-renames",
            "--date=short",
            "--pretty=format:__COMMIT__%n%H%x1f%ad%x1f%an%x1f%s",
            "--numstat",
        ])
        .output()
        .map_err(|error| format!("failed to run git log: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("git log output was not valid UTF-8: {error}"))?;

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
        .collect();
    report.directory_histories = directories
        .into_iter()
        .map(|(path, history)| (path, history.into_history()))
        .collect();

    Ok(report)
}

fn parse_commit_meta(line: &str) -> Result<CommitMeta, String> {
    let mut parts = line.split('\u{1f}');
    let hash = parts
        .next()
        .ok_or_else(|| "missing commit hash in git log output".to_string())?;
    let date = parts
        .next()
        .ok_or_else(|| "missing commit date in git log output".to_string())?;
    let author = parts
        .next()
        .ok_or_else(|| "missing commit author in git log output".to_string())?;
    let subject = parts
        .next()
        .ok_or_else(|| "missing commit subject in git log output".to_string())?;

    Ok(CommitMeta {
        hash: hash.to_string(),
        date: date.to_string(),
        author: author.to_string(),
        subject: subject.to_string(),
    })
}

fn parse_numstat_line(line: &str) -> Option<(u64, u64, String)> {
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

fn build_repo_tree(repo_path: &Path, output_dir: &Path) -> Result<TreeNode, String> {
    let excluded_output = output_dir
        .strip_prefix(repo_path)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(PathBuf::from);

    build_tree_node(repo_path, repo_path, excluded_output.as_deref())
}

fn build_tree_node(
    repo_root: &Path,
    current_path: &Path,
    excluded_output: Option<&Path>,
) -> Result<TreeNode, String> {
    let metadata = fs::symlink_metadata(current_path)
        .map_err(|error| format!("failed to read {}: {error}", current_path.display()))?;
    let relative_path = current_path
        .strip_prefix(repo_root)
        .map_err(|error| format!("failed to derive repo-relative path: {error}"))?;
    let path = path_to_string(relative_path);
    let is_dir = metadata.file_type().is_dir();
    let name = if path.is_empty() {
        repo_display_name(repo_root)
    } else {
        current_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path.as_str())
            .to_string()
    };

    let mut children = Vec::new();
    if is_dir {
        let entries = fs::read_dir(current_path).map_err(|error| {
            format!(
                "failed to read directory {}: {error}",
                current_path.display()
            )
        })?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to read an entry under {}: {error}",
                    current_path.display()
                )
            })?;
            let child_path = entry.path();
            let child_relative = child_path
                .strip_prefix(repo_root)
                .map_err(|error| format!("failed to derive repo-relative path: {error}"))?;

            if should_skip_path(child_relative, excluded_output) {
                continue;
            }

            children.push(build_tree_node(repo_root, &child_path, excluded_output)?);
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

fn write_report(output_dir: &Path, report: &RepoReport) -> Result<(), String> {
    fs::create_dir_all(output_dir)
        .map_err(|error| format!("failed to create output directory: {error}"))?;
    fs::create_dir_all(output_dir.join("files"))
        .map_err(|error| format!("failed to create files directory: {error}"))?;
    fs::create_dir_all(output_dir.join("dirs"))
        .map_err(|error| format!("failed to create dirs directory: {error}"))?;

    fs::write(output_dir.join("SUMMARY.md"), render_summary(report))
        .map_err(|error| format!("failed to write summary: {error}"))?;

    for file in sorted_histories(report.file_histories.values()) {
        let destination = markdown_path(output_dir, &file.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("failed to create directory for {}: {error}", file.path)
            })?;
        }

        fs::write(&destination, render_file_summary(file))
            .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
    }

    for directory in sorted_histories(report.directory_histories.values())
        .into_iter()
        .filter(|directory| !directory.path.is_empty())
    {
        let destination = directory_markdown_path(output_dir, &directory.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("failed to create directory for {}: {error}", directory.path)
            })?;
        }

        fs::write(&destination, render_directory_summary(directory)).map_err(|error| {
            format!(
                "failed to write directory summary {}: {error}",
                destination.display()
            )
        })?;
    }

    fs::write(output_dir.join("index.html"), render_html_viewer(report)?)
        .map_err(|error| format!("failed to write index.html: {error}"))?;

    Ok(())
}

fn markdown_path(output_dir: &Path, file_path: &str) -> PathBuf {
    let mut destination = output_dir.join("files");
    for component in Path::new(file_path).components() {
        destination.push(component);
    }
    destination.set_extension(format!(
        "{}md",
        destination
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!("{extension}."))
            .unwrap_or_default()
    ));
    destination
}

fn directory_markdown_path(output_dir: &Path, directory_path: &str) -> PathBuf {
    let mut destination = output_dir.join("dirs");
    for component in Path::new(directory_path).components() {
        destination.push(component);
    }
    destination.push("INDEX.md");
    destination
}

fn render_summary(report: &RepoReport) -> String {
    let mut markdown = String::new();
    let file_histories = sorted_histories(report.file_histories.values());
    let directory_histories = sorted_histories(report.directory_histories.values());

    writeln!(&mut markdown, "# Repository History Summary").unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "- Web viewer: [index.html](./index.html)").unwrap();
    writeln!(
        &mut markdown,
        "- Scanned commits: {}",
        report.scanned_commits
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Files with history: {}",
        report.file_histories.len()
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Folders with history: {}",
        report.directory_histories.len().saturating_sub(1)
    )
    .unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Hotspots by commit count").unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(
        &mut markdown,
        "| File | Commits | Churn | Primary authors | Note |"
    )
    .unwrap();
    writeln!(&mut markdown, "| --- | ---: | ---: | --- | --- |").unwrap();

    for file in file_histories.iter().take(SUMMARY_FILE_COUNT) {
        let churn = file.total_added + file.total_deleted;
        let authors = top_authors(file, 3);
        let note = latest_note(file);
        writeln!(
            &mut markdown,
            "| [{}](./{}) | {} | {} | {} | {} |",
            file.path,
            summary_link(&file.path),
            file.commit_count,
            churn,
            authors,
            escape_table_cell(&note)
        )
        .unwrap();
    }

    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Folders by commit count").unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(
        &mut markdown,
        "| Folder | Commits | Churn | Primary authors | Note |"
    )
    .unwrap();
    writeln!(&mut markdown, "| --- | ---: | ---: | --- | --- |").unwrap();

    for directory in directory_histories
        .iter()
        .filter(|directory| !directory.path.is_empty())
        .take(SUMMARY_DIRECTORY_COUNT)
    {
        let churn = directory.total_added + directory.total_deleted;
        let authors = top_authors(directory, 3);
        let note = latest_note(directory);
        writeln!(
            &mut markdown,
            "| [{}](./{}) | {} | {} | {} | {} |",
            directory.path,
            directory_summary_link(&directory.path),
            directory.commit_count,
            churn,
            authors,
            escape_table_cell(&note)
        )
        .unwrap();
    }

    markdown
}

fn render_file_summary(file: &PathHistory) -> String {
    let mut markdown = String::new();
    writeln!(&mut markdown, "# {}", file.path).unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "- Commits: {}", file.commit_count).unwrap();
    writeln!(
        &mut markdown,
        "- Churn: +{} / -{}",
        file.total_added, file.total_deleted
    )
    .unwrap();
    writeln!(&mut markdown, "- Primary authors: {}", top_authors(file, 5)).unwrap();
    if let Some(commit) = file.commits.first() {
        writeln!(
            &mut markdown,
            "- Latest change: {} by {} ({})",
            commit.commit.subject, commit.commit.author, commit.commit.date
        )
        .unwrap();
    }
    writeln!(&mut markdown).unwrap();
    writeln!(
        &mut markdown,
        "## Recent commits (showing up to {})",
        MARKDOWN_COMMITS_PER_NODE
    )
    .unwrap();
    writeln!(&mut markdown).unwrap();

    for commit in commit_preview(file) {
        writeln!(
            &mut markdown,
            "- `{}` {} by {} on {} (`+{}` / `-{}`)",
            short_hash(&commit.commit.hash),
            commit.commit.subject,
            commit.commit.author,
            commit.commit.date,
            commit.added,
            commit.deleted
        )
        .unwrap();
    }

    markdown
}

fn render_directory_summary(directory: &PathHistory) -> String {
    let mut markdown = String::new();
    writeln!(&mut markdown, "# Folder: {}", directory.path).unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "- Commits: {}", directory.commit_count).unwrap();
    writeln!(
        &mut markdown,
        "- Churn: +{} / -{}",
        directory.total_added, directory.total_deleted
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Primary authors: {}",
        top_authors(directory, 5)
    )
    .unwrap();
    if let Some(commit) = directory.commits.first() {
        writeln!(
            &mut markdown,
            "- Latest change: {} by {} ({})",
            commit.commit.subject, commit.commit.author, commit.commit.date
        )
        .unwrap();
    }
    writeln!(&mut markdown).unwrap();
    writeln!(
        &mut markdown,
        "## Recent commits touching this folder (showing up to {})",
        MARKDOWN_COMMITS_PER_NODE
    )
    .unwrap();
    writeln!(&mut markdown).unwrap();

    for commit in commit_preview(directory) {
        writeln!(
            &mut markdown,
            "- `{}` {} by {} on {} (`+{}` / `-{}`)",
            short_hash(&commit.commit.hash),
            commit.commit.subject,
            commit.commit.author,
            commit.commit.date,
            commit.added,
            commit.deleted
        )
        .unwrap();
    }

    markdown
}

fn render_html_viewer(report: &RepoReport) -> Result<String, String> {
    let html_data = HtmlReportData {
        repo_name: report.repo_name.clone(),
        scanned_commits: report.scanned_commits,
        changed_files: report.file_histories.len(),
        changed_directories: report.directory_histories.len().saturating_sub(1),
        nodes: collect_node_views(report),
    };
    let serialized_data = serialize_for_html(&html_data)?;

    let mut html = String::new();
    writeln!(
        &mut html,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>",
        escape_html(&format!("{} history viewer", report.repo_name))
    )
    .unwrap();
    writeln!(
        &mut html,
        "<style>{}</style>\n</head>\n<body>",
        viewer_styles()
    )
    .unwrap();
    writeln!(
        &mut html,
        "<div class=\"shell\"><aside class=\"sidebar\"><div class=\"sidebar-header\"><p class=\"eyebrow\">History to MD</p><h1>{}</h1><p class=\"meta\">{} commits scanned • {} files with history • {} folders with history</p></div><nav class=\"tree\">{}</nav></aside><main class=\"content\"><div class=\"panel\" id=\"node-details\"></div></main></div>",
        escape_html(&report.repo_name),
        report.scanned_commits,
        report.file_histories.len(),
        report.directory_histories.len().saturating_sub(1),
        render_tree_html(&report.tree, report, 0)
    )
    .unwrap();
    writeln!(
        &mut html,
        "<script id=\"report-data\" type=\"application/json\">{}</script>",
        serialized_data
    )
    .unwrap();
    writeln!(
        &mut html,
        "<script>{}</script>\n</body>\n</html>",
        viewer_script()
    )
    .unwrap();

    Ok(html)
}

fn render_tree_html(node: &TreeNode, report: &RepoReport, depth: usize) -> String {
    let history = if node.is_dir {
        report.directory_histories.get(&node.path)
    } else {
        report.file_histories.get(&node.path)
    };

    let mut markup = String::new();
    let commit_badge = history
        .filter(|history| history.commit_count > 0)
        .map(|history| {
            format!(
                "<span class=\"badge\">{} commits</span>",
                history.commit_count
            )
        })
        .unwrap_or_else(|| "<span class=\"badge badge-muted\">no history</span>".to_string());

    let button = format!(
        "<button class=\"node-button{}\" data-node-path=\"{}\" data-node-kind=\"{}\">{} {}</button>",
        if history.is_some() {
            ""
        } else {
            " node-button-muted"
        },
        escape_html_attribute(&node.path),
        if node.is_dir { "dir" } else { "file" },
        if node.is_dir {
            "<span class=\"icon\">▾</span>"
        } else {
            "<span class=\"icon\">·</span>"
        },
        escape_html(&node.name)
    );

    if node.is_dir {
        let open = if depth < 2 { " open" } else { "" };
        writeln!(
            &mut markup,
            "<details class=\"branch\"{}><summary><span class=\"summary-row\">{}{}</span></summary>",
            open, button, commit_badge
        )
        .unwrap();
        writeln!(&mut markup, "<div class=\"children\">").unwrap();
        for child in &node.children {
            markup.push_str(&render_tree_html(child, report, depth + 1));
        }
        writeln!(&mut markup, "</div></details>").unwrap();
    } else {
        writeln!(
            &mut markup,
            "<div class=\"leaf\"><span class=\"summary-row\">{}{}</span></div>",
            button, commit_badge
        )
        .unwrap();
    }

    markup
}

fn collect_node_views(report: &RepoReport) -> Vec<NodeView> {
    let mut nodes = Vec::new();
    collect_node_views_recursively(&report.tree, report, &mut nodes);
    nodes
}

fn collect_node_views_recursively(node: &TreeNode, report: &RepoReport, nodes: &mut Vec<NodeView>) {
    let history = if node.is_dir {
        report.directory_histories.get(&node.path)
    } else {
        report.file_histories.get(&node.path)
    };

    nodes.push(NodeView {
        path: node.path.clone(),
        name: node.name.clone(),
        is_dir: node.is_dir,
        commit_count: history.map(|history| history.commit_count).unwrap_or(0),
        total_added: history.map(|history| history.total_added).unwrap_or(0),
        total_deleted: history.map(|history| history.total_deleted).unwrap_or(0),
        primary_authors: history
            .map(|history| top_authors(history, 5))
            .unwrap_or_else(|| "n/a".to_string()),
        report_links: relevant_report_links(node, report),
        commits: history
            .map(|history| history.commits.clone())
            .unwrap_or_default(),
    });

    for child in &node.children {
        collect_node_views_recursively(child, report, nodes);
    }
}

fn relevant_report_links(node: &TreeNode, report: &RepoReport) -> Vec<ReportLink> {
    let mut links = Vec::new();

    if !node.is_dir && report.file_histories.contains_key(&node.path) {
        links.push(ReportLink {
            label: "File history".to_string(),
            href: summary_link(&node.path),
        });
    }

    for directory in specific_directory_chain(&node.path, node.is_dir) {
        if report.directory_histories.contains_key(&directory) {
            links.push(ReportLink {
                label: format!("Folder history: {}", display_path(&directory)),
                href: directory_summary_link(&directory),
            });
        }
    }

    links.push(ReportLink {
        label: "Repository summary".to_string(),
        href: "SUMMARY.md".to_string(),
    });

    links
}

fn top_authors(history: &PathHistory, limit: usize) -> String {
    let mut authors: Vec<(&String, &u64)> = history.authors.iter().collect();
    authors.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    let rendered = authors
        .into_iter()
        .take(limit)
        .map(|(author, count)| format!("{author} ({count})"))
        .collect::<Vec<_>>()
        .join(", ");

    if rendered.is_empty() {
        "n/a".to_string()
    } else {
        rendered
    }
}

fn latest_note(history: &PathHistory) -> String {
    history
        .commits
        .first()
        .map(|commit| format!("{} ({})", commit.commit.subject, commit.commit.date))
        .unwrap_or_else(|| "No recent commit message".to_string())
}

fn short_hash(hash: &str) -> &str {
    let hash_length = hash.len().min(8);
    &hash[..hash_length]
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn ensure_git_repository(repo_path: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|error| format!("failed to check repository: {error}"))?;

    if !output.status.success() {
        return Err(format!("not a git repository: {}", repo_path.display()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != "true" {
        return Err(format!("not a git repository: {}", repo_path.display()));
    }

    Ok(())
}

fn summary_link(file_path: &str) -> String {
    markdown_path(Path::new(""), file_path)
        .display()
        .to_string()
        .trim_start_matches("./")
        .to_string()
}

fn directory_summary_link(directory_path: &str) -> String {
    directory_markdown_path(Path::new(""), directory_path)
        .display()
        .to_string()
        .trim_start_matches("./")
        .to_string()
}

fn sorted_histories<'a>(histories: impl Iterator<Item = &'a PathHistory>) -> Vec<&'a PathHistory> {
    let mut items: Vec<_> = histories.collect();
    items.sort_by(|left, right| {
        right
            .commit_count
            .cmp(&left.commit_count)
            .then_with(|| {
                let right_churn = right.total_added + right.total_deleted;
                let left_churn = left.total_added + left.total_deleted;
                right_churn.cmp(&left_churn)
            })
            .then_with(|| left.path.cmp(&right.path))
    });
    items
}

fn ancestor_directories(file_path: &str) -> Vec<String> {
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

fn specific_directory_chain(path: &str, is_dir: bool) -> Vec<String> {
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

fn repo_display_name(repo_path: &Path) -> String {
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

fn display_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn serialize_for_html<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value)
        .map(|json| json.replace("</", "<\\/"))
        .map_err(|error| format!("failed to serialize viewer data: {error}"))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_html_attribute(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}

fn viewer_styles() -> &'static str {
    r#"
:root {
  color-scheme: light;
  --bg: #f4efe8;
  --panel: rgba(255, 251, 247, 0.92);
  --panel-strong: #fffdfa;
  --line: rgba(92, 62, 44, 0.16);
  --line-strong: rgba(92, 62, 44, 0.3);
  --text: #2f241d;
  --muted: #726153;
  --accent: #0f766e;
  --accent-soft: rgba(15, 118, 110, 0.12);
  --badge: rgba(47, 36, 29, 0.08);
  --shadow: 0 18px 40px rgba(78, 54, 38, 0.12);
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-height: 100vh;
  font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
  color: var(--text);
  background:
    radial-gradient(circle at top left, rgba(15, 118, 110, 0.12), transparent 32%),
    radial-gradient(circle at bottom right, rgba(217, 119, 6, 0.12), transparent 28%),
    var(--bg);
}

.shell {
  display: grid;
  grid-template-columns: minmax(320px, 420px) minmax(0, 1fr);
  min-height: 100vh;
  gap: 24px;
  padding: 24px;
}

.sidebar,
.panel {
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 24px;
  box-shadow: var(--shadow);
  backdrop-filter: blur(14px);
}

.sidebar {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.sidebar-header {
  padding: 24px 24px 16px;
  border-bottom: 1px solid var(--line);
}

.eyebrow {
  margin: 0 0 6px;
  font-size: 0.78rem;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--accent);
}

.sidebar-header h1 {
  margin: 0;
  font-size: clamp(1.6rem, 2.5vw, 2.2rem);
  line-height: 1.05;
}

.meta {
  margin: 10px 0 0;
  color: var(--muted);
  line-height: 1.45;
}

.tree {
  padding: 12px 16px 20px;
  overflow: auto;
}

.branch,
.leaf {
  margin: 2px 0;
}

.branch > summary {
  list-style: none;
  cursor: default;
}

.branch > summary::-webkit-details-marker {
  display: none;
}

.children {
  margin-left: 18px;
  padding-left: 10px;
  border-left: 1px solid var(--line);
}

.summary-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.node-button {
  flex: 1;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 8px 10px;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: var(--text);
  text-align: left;
  font: inherit;
  cursor: pointer;
}

.node-button:hover,
.node-button:focus-visible {
  background: rgba(47, 36, 29, 0.06);
  outline: none;
}

.node-button-selected {
  background: var(--accent-soft);
  color: var(--accent);
}

.node-button-muted {
  color: var(--muted);
}

.icon {
  width: 14px;
  color: var(--muted);
}

.badge {
  flex: none;
  white-space: nowrap;
  padding: 5px 8px;
  border-radius: 999px;
  background: var(--badge);
  color: var(--muted);
  font-size: 0.78rem;
}

.badge-muted {
  opacity: 0.8;
}

.content {
  min-width: 0;
}

.panel {
  min-height: calc(100vh - 48px);
  padding: 28px;
}

.detail-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 20px;
  margin-bottom: 20px;
}

.detail-header h2 {
  margin: 0 0 6px;
  font-size: clamp(1.5rem, 2.4vw, 2.6rem);
  line-height: 1.05;
}

.detail-path,
.empty-state {
  color: var(--muted);
}

.stat-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin: 18px 0 28px;
}

.stat-card,
.list-card {
  background: var(--panel-strong);
  border: 1px solid var(--line);
  border-radius: 18px;
  padding: 16px;
}

.stat-label {
  display: block;
  margin-bottom: 8px;
  color: var(--muted);
  font-size: 0.82rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

.stat-value {
  font-size: 1.6rem;
  font-weight: 600;
}

.section-title {
  margin: 0 0 12px;
  font-size: 1rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--muted);
}

.link-list,
.commit-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.link-list li + li,
.commit-list li + li {
  margin-top: 10px;
}

.link-list a {
  color: var(--accent);
  text-decoration: none;
}

.link-list a:hover,
.link-list a:focus-visible {
  text-decoration: underline;
}

.commit-item {
  display: grid;
  gap: 6px;
  padding: 14px 0;
  border-top: 1px solid var(--line);
}

.commit-item:first-child {
  border-top: 0;
  padding-top: 0;
}

.commit-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.hash {
  font-family: "IBM Plex Mono", "SFMono-Regular", monospace;
  color: var(--accent);
}

.commit-meta {
  color: var(--muted);
}

@media (max-width: 980px) {
  .shell {
    grid-template-columns: 1fr;
  }

  .panel {
    min-height: auto;
  }
}

@media (max-width: 640px) {
  .shell {
    padding: 16px;
    gap: 16px;
  }

  .panel,
  .sidebar {
    border-radius: 18px;
  }

  .panel {
    padding: 20px;
  }

  .stat-grid {
    grid-template-columns: 1fr;
  }
}
"#
}

fn viewer_script() -> &'static str {
    r#"
const rawData = document.getElementById("report-data").textContent;
const data = JSON.parse(rawData);
const nodeMap = new Map(data.nodes.map((node) => [node.path, node]));
const detailPanel = document.getElementById("node-details");
const buttons = Array.from(document.querySelectorAll("[data-node-path]"));

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function pluralize(count, singular, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`;
}

function renderNode(path) {
  const node = nodeMap.get(path);
  if (!node) {
    detailPanel.innerHTML = `<p class="empty-state">No data found for this node.</p>`;
    return;
  }

  buttons.forEach((button) => {
    button.classList.toggle("node-button-selected", button.dataset.nodePath === path);
  });

  const commits = node.commits.length
    ? node.commits
        .map((entry) => {
          const churn = `+${entry.added} / -${entry.deleted}`;
          return `
            <li class="commit-item">
              <div class="commit-head">
                <strong>${escapeHtml(entry.commit.subject)}</strong>
                <span class="hash">${escapeHtml(entry.commit.hash.slice(0, 8))}</span>
              </div>
              <div class="commit-meta">${escapeHtml(entry.commit.author)} • ${escapeHtml(entry.commit.date)} • ${escapeHtml(churn)}</div>
            </li>
          `;
        })
        .join("")
    : `<li class="empty-state">No git history found for this node.</li>`;

  const reportLinks = node.report_links.length
    ? node.report_links
        .map(
          (link) =>
            `<li><a href="${encodeURI(link.href)}" target="_blank" rel="noreferrer">${escapeHtml(link.label)}</a></li>`
        )
        .join("")
    : `<li class="empty-state">No generated markdown is available for this node.</li>`;

  detailPanel.innerHTML = `
    <div class="detail-header">
      <div>
        <p class="eyebrow">${node.is_dir ? "Folder" : "File"}</p>
        <h2>${escapeHtml(node.name)}</h2>
        <p class="detail-path">${escapeHtml(node.path || "/")}</p>
      </div>
      <span class="badge">${pluralize(node.commit_count, "commit")}</span>
    </div>
    <div class="stat-grid">
      <div class="stat-card">
        <span class="stat-label">Added</span>
        <strong class="stat-value">${node.total_added}</strong>
      </div>
      <div class="stat-card">
        <span class="stat-label">Deleted</span>
        <strong class="stat-value">${node.total_deleted}</strong>
      </div>
      <div class="stat-card">
        <span class="stat-label">Primary Authors</span>
        <strong class="stat-value">${escapeHtml(node.primary_authors)}</strong>
      </div>
    </div>
    <section class="list-card">
      <h3 class="section-title">Relevant Markdown</h3>
      <ul class="link-list">${reportLinks}</ul>
    </section>
    <section class="list-card" style="margin-top: 16px;">
      <h3 class="section-title">Commit History (${node.commit_count})</h3>
      <ul class="commit-list">${commits}</ul>
    </section>
  `;
}

buttons.forEach((button) => {
  button.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    renderNode(button.dataset.nodePath);
  });
});

renderNode("");
"#
}

#[cfg(test)]
mod tests {
    use super::{
        CommitMeta, HistoryAccumulator, ancestor_directories, directory_markdown_path,
        directory_summary_link, markdown_path, parse_commit_meta, parse_numstat_line,
        specific_directory_chain, summary_link,
    };
    use std::path::Path;

    #[test]
    fn parses_commit_metadata() {
        let line = "abc123\u{1f}2026-04-16\u{1f}Jane Doe\u{1f}Add parser";
        let commit = parse_commit_meta(line).expect("commit metadata should parse");
        assert_eq!(commit.hash, "abc123");
        assert_eq!(commit.date, "2026-04-16");
        assert_eq!(commit.author, "Jane Doe");
        assert_eq!(commit.subject, "Add parser");
    }

    #[test]
    fn parses_numstat_lines() {
        let change = parse_numstat_line("12\t4\tsrc/main.rs").expect("numstat should parse");
        assert_eq!(change.0, 12);
        assert_eq!(change.1, 4);
        assert_eq!(change.2, "src/main.rs");
    }

    #[test]
    fn builds_markdown_path() {
        let path = markdown_path(Path::new("history-md"), "src/main.rs");
        assert_eq!(path, Path::new("history-md/files/src/main.rs.md"));

        let path = markdown_path(Path::new("history-md"), "Makefile");
        assert_eq!(path, Path::new("history-md/files/Makefile.md"));
    }

    #[test]
    fn builds_directory_markdown_path() {
        let path = directory_markdown_path(Path::new("history-md"), "src/components");
        assert_eq!(path, Path::new("history-md/dirs/src/components/INDEX.md"));
    }

    #[test]
    fn builds_summary_links() {
        assert_eq!(summary_link("src/main.rs"), "files/src/main.rs.md");
        assert_eq!(summary_link("README.md"), "files/README.md.md");
        assert_eq!(
            directory_summary_link("src/components"),
            "dirs/src/components/INDEX.md"
        );
    }

    #[test]
    fn collects_ancestor_directories() {
        assert_eq!(ancestor_directories("README.md"), vec![""]);
        assert_eq!(ancestor_directories("src/main.rs"), vec!["", "src"]);
        assert_eq!(
            ancestor_directories("src/components/button.rs"),
            vec!["", "src", "src/components"]
        );
    }

    #[test]
    fn builds_specific_directory_chain() {
        assert_eq!(
            specific_directory_chain("README.md", false),
            Vec::<String>::new()
        );
        assert_eq!(
            specific_directory_chain("src/main.rs", false),
            vec!["src".to_string()]
        );
        assert_eq!(
            specific_directory_chain("src/components", true),
            vec!["src/components".to_string(), "src".to_string()]
        );
    }

    #[test]
    fn aggregates_multiple_changes_from_same_commit() {
        let commit = CommitMeta {
            hash: "abc123".to_string(),
            date: "2026-04-16".to_string(),
            author: "Jane Doe".to_string(),
            subject: "Update folder".to_string(),
        };
        let mut history = HistoryAccumulator::new("src".to_string());

        history.record_change(&commit, 5, 1);
        history.record_change(&commit, 3, 2);

        let history = history.into_history();
        assert_eq!(history.commit_count, 1);
        assert_eq!(history.commits.len(), 1);
        assert_eq!(history.commits[0].added, 8);
        assert_eq!(history.commits[0].deleted, 3);
    }
}
