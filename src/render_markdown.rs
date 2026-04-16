use crate::git_history::latest_activity_histories;
use crate::model::{
    AgentProfile, DetectedTechnology, MARKDOWN_COMMITS_PER_NODE, PathHistory, RepoReport,
    SUMMARY_DIRECTORY_COUNT, SUMMARY_FILE_COUNT, commit_preview, directory_markdown_path,
    markdown_path,
};
use std::fmt::Write as _;
use std::path::Path;

pub fn render_summary(report: &RepoReport) -> String {
    let mut markdown = String::new();
    let file_histories = report.sorted_file_histories();
    let directory_histories = report.sorted_directory_histories();

    write_markdown_frontmatter(
        &mut markdown,
        report,
        "repo-summary",
        "Repository History Summary",
        None,
    );
    writeln!(&mut markdown, "# Repository History Summary").unwrap();
    writeln!(&mut markdown).unwrap();
    write_agent_format_section(&mut markdown, report.agent_profile);
    writeln!(&mut markdown, "## Snapshot").unwrap();
    writeln!(&mut markdown).unwrap();
    if report.output_formats.includes_html() {
        writeln!(&mut markdown, "- Web viewer: [index.html](./index.html)").unwrap();
    } else {
        writeln!(&mut markdown, "- Web viewer: not generated").unwrap();
    }
    if report.output_formats.includes_json() {
        writeln!(
            &mut markdown,
            "- Machine report: [report.json](./report.json)"
        )
        .unwrap();
    } else {
        writeln!(&mut markdown, "- Machine report: not generated").unwrap();
    }
    writeln!(
        &mut markdown,
        "- Agent profile: {}",
        report.agent_profile.display_name()
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Output formats: {}",
        report.output_formats.to_labels().join(", ")
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Scanned commits: {}",
        report.scanned_commits
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Files with history: {}",
        report.changed_files()
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Folders with history: {}",
        report.changed_directories()
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Include filters: {}",
        render_filters(report)
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Detected technologies: {}",
        format_detected_technologies(&report.detected_technologies)
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Added skills: {}",
        if report.added_skills.is_empty() {
            "none".to_string()
        } else {
            report
                .added_skills
                .iter()
                .map(|skill| skill.title.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    )
    .unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Technology detection").unwrap();
    writeln!(&mut markdown).unwrap();
    if report.detected_technologies.is_empty() {
        writeln!(&mut markdown, "- No technologies detected.").unwrap();
    } else {
        for technology in &report.detected_technologies {
            writeln!(
                &mut markdown,
                "- {}: {}",
                technology.name,
                technology.evidence.join(", ")
            )
            .unwrap();
        }
    }
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Skills from database").unwrap();
    writeln!(&mut markdown).unwrap();
    if report.added_skills.is_empty() {
        writeln!(
            &mut markdown,
            "- No matching skills were added from a skills database."
        )
        .unwrap();
    } else {
        for skill in &report.added_skills {
            let location = markdown_link_or_code(&skill.location, skill.href.as_deref());
            writeln!(
                &mut markdown,
                "- {}: {} Matched technologies: {}. Installed at {}.",
                skill.title,
                skill.description,
                skill.matched_technologies.join(", "),
                location
            )
            .unwrap();
        }
        if let Some(manifest_href) = report.skills_manifest_href.as_deref() {
            writeln!(
                &mut markdown,
                "- Skills manifest: [manifest.json](./{})",
                manifest_href
            )
            .unwrap();
        }
    }
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Hotspots by commit count").unwrap();
    writeln!(&mut markdown).unwrap();
    render_history_table(
        &mut markdown,
        "File",
        file_histories.iter().copied().take(SUMMARY_FILE_COUNT),
        true,
    );
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Hotspots by churn").unwrap();
    writeln!(&mut markdown).unwrap();
    let mut by_churn = file_histories.clone();
    by_churn.sort_by(|left, right| {
        (right.total_added + right.total_deleted)
            .cmp(&(left.total_added + left.total_deleted))
            .then_with(|| right.commit_count.cmp(&left.commit_count))
            .then_with(|| left.path.cmp(&right.path))
    });
    render_history_table(
        &mut markdown,
        "File",
        by_churn.iter().copied().take(SUMMARY_FILE_COUNT),
        true,
    );
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Ownership concentration").unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(
        &mut markdown,
        "| Path | Primary author | Share of commits | Total commits |"
    )
    .unwrap();
    writeln!(&mut markdown, "| --- | --- | ---: | ---: |").unwrap();
    for file in file_histories.iter().take(SUMMARY_FILE_COUNT) {
        let (author, share) = ownership_concentration(file);
        writeln!(
            &mut markdown,
            "| {} | {} | {} | {} |",
            linked_history_path(file, true),
            author,
            share,
            file.commit_count
        )
        .unwrap();
    }
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Recent activity").unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(
        &mut markdown,
        "| Path | Latest commit date | Latest note | Commits |"
    )
    .unwrap();
    writeln!(&mut markdown, "| --- | --- | --- | ---: |").unwrap();
    for file in latest_activity_histories(report.file_histories.values())
        .into_iter()
        .take(SUMMARY_FILE_COUNT)
    {
        let latest_date = file
            .commits
            .first()
            .map(|commit| commit.commit.date.as_str())
            .unwrap_or("n/a");
        writeln!(
            &mut markdown,
            "| {} | {} | {} | {} |",
            linked_history_path(file, true),
            latest_date,
            escape_table_cell(&latest_note(file)),
            file.commit_count
        )
        .unwrap();
    }
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Folders by commit count").unwrap();
    writeln!(&mut markdown).unwrap();
    render_history_table(
        &mut markdown,
        "Folder",
        directory_histories
            .iter()
            .filter(|directory| !directory.path.is_empty())
            .copied()
            .take(SUMMARY_DIRECTORY_COUNT),
        false,
    );

    markdown
}

pub fn render_file_summary(report: &RepoReport, file: &PathHistory) -> String {
    let mut markdown = String::new();
    write_markdown_frontmatter(
        &mut markdown,
        report,
        "file-history",
        &file.path,
        Some(&file.path),
    );
    writeln!(&mut markdown, "# {}", file.path).unwrap();
    writeln!(&mut markdown).unwrap();
    write_agent_format_section(&mut markdown, report.agent_profile);
    writeln!(&mut markdown, "## Snapshot").unwrap();
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

pub fn render_directory_summary(report: &RepoReport, directory: &PathHistory) -> String {
    let mut markdown = String::new();
    write_markdown_frontmatter(
        &mut markdown,
        report,
        "directory-history",
        &format!("Folder: {}", directory.path),
        Some(&directory.path),
    );
    writeln!(&mut markdown, "# Folder: {}", directory.path).unwrap();
    writeln!(&mut markdown).unwrap();
    write_agent_format_section(&mut markdown, report.agent_profile);
    writeln!(&mut markdown, "## Snapshot").unwrap();
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

pub fn summary_link(file_path: &str) -> String {
    markdown_path(Path::new(""), file_path)
        .display()
        .to_string()
        .trim_start_matches("./")
        .to_string()
}

pub fn directory_summary_link(directory_path: &str) -> String {
    directory_markdown_path(Path::new(""), directory_path)
        .display()
        .to_string()
        .trim_start_matches("./")
        .to_string()
}

pub fn top_authors(history: &PathHistory, limit: usize) -> String {
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

pub fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn latest_note(history: &PathHistory) -> String {
    history
        .commits
        .first()
        .map(|commit| format!("{} ({})", commit.commit.subject, commit.commit.date))
        .unwrap_or_else(|| "No recent commit message".to_string())
}

fn render_history_table<'a>(
    markdown: &mut String,
    heading: &str,
    histories: impl Iterator<Item = &'a PathHistory>,
    is_file: bool,
) {
    writeln!(
        markdown,
        "| {heading} | Commits | Churn | Primary authors | Note |"
    )
    .unwrap();
    writeln!(markdown, "| --- | ---: | ---: | --- | --- |").unwrap();

    for history in histories {
        let churn = history.total_added + history.total_deleted;
        let authors = top_authors(history, 3);
        let note = latest_note(history);
        writeln!(
            markdown,
            "| {} | {} | {} | {} | {} |",
            linked_history_path(history, is_file),
            history.commit_count,
            churn,
            authors,
            escape_table_cell(&note)
        )
        .unwrap();
    }
}

fn linked_history_path(history: &PathHistory, is_file: bool) -> String {
    if is_file {
        format!("[{}](./{})", history.path, summary_link(&history.path))
    } else {
        format!(
            "[{}](./{})",
            history.path,
            directory_summary_link(&history.path)
        )
    }
}

fn ownership_concentration(history: &PathHistory) -> (String, String) {
    let Some((author, count)) = history
        .authors
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
    else {
        return ("n/a".to_string(), "0%".to_string());
    };

    let share = if history.commit_count == 0 {
        0.0
    } else {
        (*count as f64 / history.commit_count as f64) * 100.0
    };

    (author.clone(), format!("{share:.0}%"))
}

fn render_filters(report: &RepoReport) -> String {
    let mut pieces = Vec::new();
    if !report.include_patterns.is_empty() {
        pieces.push(format!("include {}", report.include_patterns.join(", ")));
    }
    if !report.exclude_patterns.is_empty() {
        pieces.push(format!("exclude {}", report.exclude_patterns.join(", ")));
    }
    if pieces.is_empty() {
        "none".to_string()
    } else {
        pieces.join("; ")
    }
}

fn write_markdown_frontmatter(
    markdown: &mut String,
    report: &RepoReport,
    document_kind: &str,
    title: &str,
    path: Option<&str>,
) {
    writeln!(markdown, "---").unwrap();
    writeln!(markdown, "generated_by: history-to-md").unwrap();
    writeln!(markdown, "format_version: 1").unwrap();
    writeln!(markdown, "agent_profile: {}", report.agent_profile.slug()).unwrap();
    writeln!(
        markdown,
        "agent_display_name: {}",
        yaml_string(report.agent_profile.display_name())
    )
    .unwrap();
    writeln!(markdown, "document_kind: {document_kind}").unwrap();
    writeln!(markdown, "repo_name: {}", yaml_string(&report.repo_name)).unwrap();
    writeln!(markdown, "title: {}", yaml_string(title)).unwrap();
    if let Some(path) = path {
        writeln!(markdown, "path: {}", yaml_string(path)).unwrap();
    }
    writeln!(markdown, "---").unwrap();
    writeln!(markdown).unwrap();
}

fn write_agent_format_section(markdown: &mut String, agent_profile: AgentProfile) {
    writeln!(markdown, "## Agent Format").unwrap();
    writeln!(markdown).unwrap();
    writeln!(markdown, "- Target agent: {}", agent_profile.display_name()).unwrap();
    writeln!(
        markdown,
        "- Markdown style: {}",
        agent_profile.markdown_style()
    )
    .unwrap();
    writeln!(markdown, "- Usage hint: {}", agent_profile.usage_hint()).unwrap();
    writeln!(
        markdown,
        "- Relative links: All markdown links stay relative to the generated history output directory."
    )
    .unwrap();
    writeln!(markdown).unwrap();
}

fn format_detected_technologies(technologies: &[DetectedTechnology]) -> String {
    if technologies.is_empty() {
        "none".to_string()
    } else {
        technologies
            .iter()
            .map(|technology| technology.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn markdown_link_or_code(label: &str, href: Option<&str>) -> String {
    match href {
        Some(href) => format!("[`{label}`](./{href})"),
        None => format!("`{label}`"),
    }
}

fn short_hash(hash: &str) -> &str {
    let hash_length = hash.len().min(8);
    &hash[..hash_length]
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|")
}
