use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const GENERATED_BY: &str = "history-to-md";
pub const FORMAT_VERSION: u32 = 1;
pub const DEFAULT_OUTPUT_DIR: &str = "history-md";
pub const MARKDOWN_COMMITS_PER_NODE: usize = 12;
pub const SUMMARY_FILE_COUNT: usize = 20;
pub const SUMMARY_DIRECTORY_COUNT: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentProfile {
    Generic,
    Codex,
    Claude,
    Cursor,
    Aider,
}

impl AgentProfile {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "generic" => Ok(Self::Generic),
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "cursor" => Ok(Self::Cursor),
            "aider" => Ok(Self::Aider),
            _ => Err(format!(
                "unknown agent profile: {value}\nsupported agent profiles: {}",
                Self::supported_names().join(", ")
            )),
        }
    }

    pub fn supported_names() -> &'static [&'static str] {
        &["generic", "codex", "claude", "cursor", "aider"]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Aider => "aider",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Generic => "Generic Agent",
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Cursor => "Cursor",
            Self::Aider => "Aider",
        }
    }

    pub fn markdown_style(self) -> &'static str {
        match self {
            Self::Generic => {
                "Structured headings with concise bullets and relative links to related history files."
            }
            Self::Codex => {
                "Direct engineering-oriented sections, flat bullets, and code identifiers kept in backticks."
            }
            Self::Claude => {
                "Short explanatory sections with explicit headings and slightly more contextual prose."
            }
            Self::Cursor => {
                "Compact IDE-friendly sections optimized for quick scanning in a sidebar or editor pane."
            }
            Self::Aider => {
                "Terse patch-oriented notes that put actionable facts and recent changes near the top."
            }
        }
    }

    pub fn usage_hint(self) -> &'static str {
        match self {
            Self::Generic => {
                "Use when you only need stable markdown summaries without agent-specific tuning."
            }
            Self::Codex => {
                "Use when the reader is a coding agent that prefers terse, operational context."
            }
            Self::Claude => {
                "Use when the reader benefits from a little more narrative framing around the raw history."
            }
            Self::Cursor => {
                "Use when the markdown will mainly be inspected alongside the repo inside an editor."
            }
            Self::Aider => {
                "Use when the markdown is feeding a code-editing loop that needs quick implementation context."
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DetectedTechnology {
    pub id: String,
    pub name: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AddedSkill {
    pub id: String,
    pub title: String,
    pub description: String,
    pub matched_technologies: Vec<String>,
    pub location: String,
    pub href: Option<String>,
}

#[derive(Debug, Default)]
pub struct SkillsIntegration {
    pub added_skills: Vec<AddedSkill>,
    pub skills_manifest_href: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CommitMeta {
    pub hash: String,
    pub date: String,
    pub author: String,
    pub subject: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FileCommit {
    pub commit: CommitMeta,
    pub added: u64,
    pub deleted: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PathHistory {
    pub path: String,
    pub commit_count: u64,
    pub total_added: u64,
    pub total_deleted: u64,
    pub authors: BTreeMap<String, u64>,
    pub commits: Vec<FileCommit>,
}

#[derive(Debug, Default)]
pub struct HistoryAccumulator {
    path: String,
    commit_count: u64,
    total_added: u64,
    total_deleted: u64,
    authors: BTreeMap<String, u64>,
    commits: Vec<FileCommit>,
    commit_indices: HashMap<String, usize>,
}

impl HistoryAccumulator {
    pub fn new(path: String) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }

    pub fn record_change(&mut self, commit: &CommitMeta, added: u64, deleted: u64) {
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

    pub fn into_history(self) -> PathHistory {
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

#[derive(Debug, Default)]
pub struct HistoryReport {
    pub scanned_commits: u64,
    pub file_histories: BTreeMap<String, PathHistory>,
    pub directory_histories: BTreeMap<String, PathHistory>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TreeNode {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Debug)]
pub struct RepoReport {
    pub repo_name: String,
    pub scanned_commits: u64,
    pub file_histories: BTreeMap<String, PathHistory>,
    pub directory_histories: BTreeMap<String, PathHistory>,
    pub tree: TreeNode,
    pub agent_profile: AgentProfile,
    pub detected_technologies: Vec<DetectedTechnology>,
    pub added_skills: Vec<AddedSkill>,
    pub skills_manifest_href: Option<String>,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub output_formats: OutputFormats,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReportBundle {
    pub generated_by: &'static str,
    pub format_version: u32,
    pub repo_name: String,
    pub agent_profile: AgentProfile,
    pub agent_display_name: String,
    pub scanned_commits: u64,
    pub detected_technologies: Vec<DetectedTechnology>,
    pub added_skills: Vec<AddedSkill>,
    pub skills_manifest_href: Option<String>,
    pub tree: TreeNode,
    pub file_histories: Vec<PathHistory>,
    pub directory_histories: Vec<PathHistory>,
    pub available_formats: Vec<String>,
}

impl RepoReport {
    pub fn file_history(&self, path: &str) -> Option<&PathHistory> {
        self.file_histories.get(path)
    }

    pub fn directory_history(&self, path: &str) -> Option<&PathHistory> {
        self.directory_histories.get(path)
    }

    pub fn changed_files(&self) -> usize {
        self.file_histories.len()
    }

    pub fn changed_directories(&self) -> usize {
        self.directory_histories.len().saturating_sub(1)
    }

    pub fn sorted_file_histories(&self) -> Vec<&PathHistory> {
        sorted_histories(self.file_histories.values())
    }

    pub fn sorted_directory_histories(&self) -> Vec<&PathHistory> {
        sorted_histories(self.directory_histories.values())
    }

    pub fn to_bundle(&self) -> ReportBundle {
        ReportBundle {
            generated_by: GENERATED_BY,
            format_version: FORMAT_VERSION,
            repo_name: self.repo_name.clone(),
            agent_profile: self.agent_profile,
            agent_display_name: self.agent_profile.display_name().to_string(),
            scanned_commits: self.scanned_commits,
            detected_technologies: self.detected_technologies.clone(),
            added_skills: self.added_skills.clone(),
            skills_manifest_href: self.skills_manifest_href.clone(),
            tree: self.tree.clone(),
            file_histories: self.sorted_file_histories().into_iter().cloned().collect(),
            directory_histories: self
                .sorted_directory_histories()
                .into_iter()
                .cloned()
                .collect(),
            available_formats: self.output_formats.to_labels(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GenerationOptions {
    pub since: Option<String>,
    pub until: Option<String>,
    pub max_commits: Option<usize>,
    pub matcher: PathMatcher,
}

#[derive(Clone, Debug)]
pub struct PathMatcher {
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    include_set: Option<globset::GlobSet>,
    exclude_set: globset::GlobSet,
}

impl PathMatcher {
    pub fn new(
        include_patterns: Vec<String>,
        exclude_patterns: Vec<String>,
    ) -> Result<Self, String> {
        let include_set = if include_patterns.is_empty() {
            None
        } else {
            Some(build_glob_set(&include_patterns)?)
        };
        let exclude_set = build_glob_set(&exclude_patterns)?;
        Ok(Self {
            include_patterns,
            exclude_patterns,
            include_set,
            exclude_set,
        })
    }

    pub fn include_patterns(&self) -> &[String] {
        &self.include_patterns
    }

    pub fn exclude_patterns(&self) -> &[String] {
        &self.exclude_patterns
    }

    pub fn matches_file(&self, path: &str) -> bool {
        self.matches(path)
    }

    pub fn include_all(&self) -> bool {
        self.include_set.is_none() && self.exclude_patterns.is_empty()
    }

    pub fn keep_dir(&self, path: &str, has_children: bool) -> bool {
        if path.is_empty() {
            return true;
        }
        if self.exclude_set.is_match(path) {
            return false;
        }
        has_children
            || self
                .include_set
                .as_ref()
                .is_none_or(|set| set.is_match(path))
    }

    fn matches(&self, path: &str) -> bool {
        let included = self
            .include_set
            .as_ref()
            .is_none_or(|set| set.is_match(path));
        included && !self.exclude_set.is_match(path)
    }
}

impl Default for PathMatcher {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new()).expect("default matcher should compile")
    }
}

fn build_glob_set(patterns: &[String]) -> Result<globset::GlobSet, String> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            globset::Glob::new(pattern)
                .map_err(|error| format!("invalid glob pattern `{pattern}`: {error}"))?,
        );
    }
    builder
        .build()
        .map_err(|error| format!("failed to compile glob patterns: {error}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormats {
    markdown: bool,
    html: bool,
    json: bool,
}

impl OutputFormats {
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut formats = Self {
            markdown: false,
            html: false,
            json: false,
        };

        for raw_part in value.split(',') {
            let part = raw_part.trim();
            match part {
                "md" => formats.markdown = true,
                "html" => formats.html = true,
                "json" => formats.json = true,
                "" => return Err("output formats cannot contain empty entries".to_string()),
                _ => {
                    return Err(format!(
                        "unknown output format: {part}\nsupported output formats: md, html, json"
                    ));
                }
            }
        }

        if !formats.markdown && !formats.html && !formats.json {
            return Err("at least one output format must be enabled".to_string());
        }

        Ok(formats)
    }

    pub fn includes_markdown(self) -> bool {
        self.markdown
    }

    pub fn includes_html(self) -> bool {
        self.html
    }

    pub fn includes_json(self) -> bool {
        self.json
    }

    pub fn to_labels(self) -> Vec<String> {
        let mut labels = Vec::new();
        if self.markdown {
            labels.push("md".to_string());
        }
        if self.html {
            labels.push("html".to_string());
        }
        if self.json {
            labels.push("json".to_string());
        }
        labels
    }
}

impl Default for OutputFormats {
    fn default() -> Self {
        Self {
            markdown: true,
            html: true,
            json: true,
        }
    }
}

pub fn commit_preview(history: &PathHistory) -> impl Iterator<Item = &FileCommit> {
    history.commits.iter().take(MARKDOWN_COMMITS_PER_NODE)
}

pub fn markdown_path(output_dir: &Path, file_path: &str) -> PathBuf {
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

pub fn directory_markdown_path(output_dir: &Path, directory_path: &str) -> PathBuf {
    let mut destination = output_dir.join("dirs");
    for component in Path::new(directory_path).components() {
        destination.push(component);
    }
    destination.push("INDEX.md");
    destination
}

pub fn sorted_histories<'a>(
    histories: impl Iterator<Item = &'a PathHistory>,
) -> Vec<&'a PathHistory> {
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
