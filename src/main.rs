use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    let tree = build_repo_tree(&config.repo_path, &config.output_dir)?;
    let detected_technologies = detect_technologies(&config.repo_path, &tree)?;
    let skills_result = match config.skills_database.as_ref() {
        Some(skills_config) => {
            add_skills_from_database(skills_config, &config.output_dir, &detected_technologies)?
        }
        None => SkillsIntegration::default(),
    };
    let report = RepoReport {
        repo_name: repo_display_name(&config.repo_path),
        scanned_commits: history.scanned_commits,
        file_histories: history.file_histories,
        directory_histories: history.directory_histories,
        tree,
        agent_profile: config.agent_profile,
        detected_technologies,
        added_skills: skills_result.added_skills,
        skills_manifest_href: skills_result.skills_manifest_href,
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
    agent_profile: AgentProfile,
    skills_database: Option<SkillsDatabaseConfig>,
}

#[derive(Clone, Debug)]
struct SkillsDatabaseConfig {
    database_path: PathBuf,
    install_dir: PathBuf,
}

impl Config {
    fn from_args(args: &[String]) -> Result<Self, String> {
        let program_name = args.first().map_or("history-to-md", String::as_str);
        let mut positionals = Vec::new();
        let mut agent_profile = AgentProfile::Generic;
        let mut skills_database_path: Option<PathBuf> = None;
        let mut skills_install_dir: Option<PathBuf> = None;
        let mut index = 1;

        while index < args.len() {
            match args[index].as_str() {
                "--agent" => {
                    let Some(value) = args.get(index + 1) else {
                        return Err(format!(
                            "missing value for --agent\n{}",
                            usage(program_name)
                        ));
                    };
                    agent_profile = AgentProfile::parse(value)?;
                    index += 2;
                }
                "--skills-db" => {
                    let Some(value) = args.get(index + 1) else {
                        return Err(format!(
                            "missing value for --skills-db\n{}",
                            usage(program_name)
                        ));
                    };
                    skills_database_path = Some(PathBuf::from(value));
                    index += 2;
                }
                "--skills-dir" => {
                    let Some(value) = args.get(index + 1) else {
                        return Err(format!(
                            "missing value for --skills-dir\n{}",
                            usage(program_name)
                        ));
                    };
                    skills_install_dir = Some(PathBuf::from(value));
                    index += 2;
                }
                argument if argument.starts_with("--") => {
                    return Err(format!(
                        "unknown option: {argument}\n{}",
                        usage(program_name)
                    ));
                }
                argument => {
                    positionals.push(argument.to_string());
                    index += 1;
                }
            }
        }

        if positionals.len() < 1 || positionals.len() > 2 {
            return Err(usage(program_name));
        }

        let repo_path = PathBuf::from(&positionals[0]);
        if !repo_path.exists() {
            return Err(format!(
                "repository path does not exist: {}",
                repo_path.display()
            ));
        }

        let output_dir = positionals
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_path.join(DEFAULT_OUTPUT_DIR));

        let skills_database = match skills_database_path {
            Some(database_path) => {
                if !database_path.exists() {
                    return Err(format!(
                        "skills database path does not exist: {}",
                        database_path.display()
                    ));
                }

                let install_dir = skills_install_dir.unwrap_or_else(|| output_dir.join("skills"));
                Some(SkillsDatabaseConfig {
                    database_path,
                    install_dir,
                })
            }
            None => {
                if let Some(install_dir) = skills_install_dir {
                    return Err(format!(
                        "--skills-dir requires --skills-db (got {})",
                        install_dir.display()
                    ));
                }
                None
            }
        };

        Ok(Self {
            repo_path,
            output_dir,
            agent_profile,
            skills_database,
        })
    }
}

fn usage(program_name: &str) -> String {
    format!(
        "usage: {program_name} [--agent <{}>] [--skills-db <path>] [--skills-dir <path>] <repo-path> [output-dir]",
        AgentProfile::supported_names().join("|")
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AgentProfile {
    Generic,
    Codex,
    Claude,
    Cursor,
    Aider,
}

impl AgentProfile {
    fn parse(value: &str) -> Result<Self, String> {
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

    fn supported_names() -> &'static [&'static str] {
        &["generic", "codex", "claude", "cursor", "aider"]
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Aider => "aider",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Generic => "Generic Agent",
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Cursor => "Cursor",
            Self::Aider => "Aider",
        }
    }

    fn markdown_style(self) -> &'static str {
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

    fn usage_hint(self) -> &'static str {
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
struct DetectedTechnology {
    id: String,
    name: String,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct AddedSkill {
    id: String,
    title: String,
    description: String,
    matched_technologies: Vec<String>,
    location: String,
    href: Option<String>,
}

#[derive(Debug, Default)]
struct SkillsIntegration {
    added_skills: Vec<AddedSkill>,
    skills_manifest_href: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillsDatabase {
    skills: Vec<SkillsDatabaseEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct SkillsDatabaseEntry {
    id: String,
    title: String,
    description: String,
    technologies: Vec<String>,
    #[serde(default)]
    match_mode: SkillMatchMode,
    source: Option<String>,
    install_as: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SkillMatchMode {
    #[default]
    Any,
    All,
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
    agent_profile: AgentProfile,
    detected_technologies: Vec<DetectedTechnology>,
    added_skills: Vec<AddedSkill>,
    skills_manifest_href: Option<String>,
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
    agent_profile: String,
    scanned_commits: u64,
    changed_files: usize,
    changed_directories: usize,
    detected_technologies: Vec<DetectedTechnology>,
    added_skills: Vec<AddedSkill>,
    skills_manifest_href: Option<String>,
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

fn detect_technologies(
    repo_path: &Path,
    tree: &TreeNode,
) -> Result<Vec<DetectedTechnology>, String> {
    let mut files = Vec::new();
    collect_file_paths(tree, &mut files);
    let file_set: HashSet<&str> = files.iter().map(String::as_str).collect();
    let mut detected = Vec::new();

    push_detected_technology(
        &mut detected,
        "docker",
        "Docker",
        vec![
            find_exact_path(&file_set, &files, "Dockerfile"),
            find_suffix_path(&files, ".dockerfile"),
            find_exact_path(&file_set, &files, "docker-compose.yml"),
            find_exact_path(&file_set, &files, "docker-compose.yaml"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "go",
        "Go",
        vec![
            find_exact_path(&file_set, &files, "go.mod"),
            find_path_with_extension(&files, "go"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "java",
        "Java",
        vec![
            find_exact_path(&file_set, &files, "pom.xml"),
            find_exact_path(&file_set, &files, "build.gradle"),
            find_path_with_extension(&files, "java"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "javascript",
        "JavaScript",
        vec![
            find_exact_path(&file_set, &files, "package.json"),
            find_path_with_extension(&files, "js"),
            find_path_with_extension(&files, "jsx"),
            find_path_with_extension(&files, "mjs"),
            find_path_with_extension(&files, "cjs"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "kotlin",
        "Kotlin",
        vec![
            find_exact_path(&file_set, &files, "build.gradle.kts"),
            find_path_with_extension(&files, "kt"),
            find_path_with_extension(&files, "kts"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "kubernetes",
        "Kubernetes",
        vec![
            find_exact_path(&file_set, &files, "Chart.yaml"),
            find_exact_path(&file_set, &files, "kustomization.yaml"),
            find_exact_path(&file_set, &files, "kustomization.yml"),
            find_prefix_path(&files, "k8s/"),
            find_prefix_path(&files, "helm/"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "nodejs",
        "Node.js",
        vec![find_exact_path(&file_set, &files, "package.json")],
    );
    push_detected_technology(
        &mut detected,
        "python",
        "Python",
        vec![
            find_exact_path(&file_set, &files, "pyproject.toml"),
            find_exact_path(&file_set, &files, "requirements.txt"),
            find_exact_path(&file_set, &files, "setup.py"),
            find_path_with_extension(&files, "py"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "rust",
        "Rust",
        vec![
            find_exact_path(&file_set, &files, "Cargo.toml"),
            find_path_with_extension(&files, "rs"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "terraform",
        "Terraform",
        vec![
            find_path_with_extension(&files, "tf"),
            find_path_with_extension(&files, "tfvars"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "typescript",
        "TypeScript",
        vec![
            find_exact_path(&file_set, &files, "tsconfig.json"),
            find_path_with_extension(&files, "ts"),
            find_path_with_extension(&files, "tsx"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "react",
        "React",
        vec![
            file_contains_any(
                repo_path,
                "package.json",
                &["\"react\"", "\"next\"", "\"@types/react\""],
            )?,
            find_path_with_extension(&files, "jsx"),
            find_path_with_extension(&files, "tsx"),
        ],
    );

    detected.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(detected)
}

fn add_skills_from_database(
    config: &SkillsDatabaseConfig,
    output_dir: &Path,
    detected_technologies: &[DetectedTechnology],
) -> Result<SkillsIntegration, String> {
    let database = load_skills_database(&config.database_path)?;
    let detected_ids: HashSet<&str> = detected_technologies
        .iter()
        .map(|tech| tech.id.as_str())
        .collect();
    let database_root = config
        .database_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut added_skills = Vec::new();

    fs::create_dir_all(&config.install_dir).map_err(|error| {
        format!(
            "failed to create skills install directory {}: {error}",
            config.install_dir.display()
        )
    })?;

    for skill in database.skills {
        let matched_ids = matched_technology_ids(&skill, &detected_ids);
        if matched_ids.is_empty() {
            continue;
        }

        let matched_technologies = detected_technologies
            .iter()
            .filter(|technology| matched_ids.iter().any(|id| *id == technology.id))
            .map(|technology| technology.name.clone())
            .collect::<Vec<_>>();

        let location_name = skill
            .install_as
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| skill.id.clone());
        let destination = config.install_dir.join(&location_name);
        let source = skill.source.as_deref().ok_or_else(|| {
            format!(
                "matched skill '{}' is missing a source path in {}",
                skill.id,
                config.database_path.display()
            )
        })?;
        let source_path = database_root.join(source);
        let source_metadata = fs::metadata(&source_path).map_err(|error| {
            format!(
                "failed to inspect skill source {}: {error}",
                source_path.display()
            )
        })?;
        copy_path_recursively(&source_path, &destination)?;
        let linked_location = preferred_skill_link_target(&destination, source_metadata.is_dir());

        added_skills.push(AddedSkill {
            id: skill.id,
            title: skill.title,
            description: skill.description,
            matched_technologies,
            location: display_skill_location(&linked_location),
            href: relative_href(output_dir, &linked_location),
        });
    }

    let skills_manifest_href = if !added_skills.is_empty() {
        let manifest_path = config.install_dir.join("manifest.json");
        fs::write(
            &manifest_path,
            render_skills_manifest(detected_technologies, &added_skills)?,
        )
        .map_err(|error| {
            format!(
                "failed to write skills manifest {}: {error}",
                manifest_path.display()
            )
        })?;
        relative_href(output_dir, &manifest_path)
    } else {
        None
    };

    Ok(SkillsIntegration {
        added_skills,
        skills_manifest_href,
    })
}

fn load_skills_database(database_path: &Path) -> Result<SkillsDatabase, String> {
    let contents = fs::read_to_string(database_path).map_err(|error| {
        format!(
            "failed to read skills database {}: {error}",
            database_path.display()
        )
    })?;

    serde_json::from_str(&contents).map_err(|error| {
        format!(
            "failed to parse skills database {}: {error}",
            database_path.display()
        )
    })
}

fn matched_technology_ids<'a>(
    skill: &'a SkillsDatabaseEntry,
    detected_ids: &HashSet<&str>,
) -> Vec<&'a str> {
    let normalized_ids = skill
        .technologies
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    let matches = normalized_ids
        .iter()
        .copied()
        .filter(|technology| detected_ids.contains(technology))
        .collect::<Vec<_>>();

    match skill.match_mode {
        SkillMatchMode::Any => matches,
        SkillMatchMode::All if matches.len() == normalized_ids.len() => matches,
        SkillMatchMode::All => Vec::new(),
    }
}

fn render_skills_manifest(
    detected_technologies: &[DetectedTechnology],
    added_skills: &[AddedSkill],
) -> Result<String, String> {
    #[derive(Serialize)]
    struct SkillsManifest<'a> {
        generated_by: &'static str,
        detected_technologies: &'a [DetectedTechnology],
        added_skills: &'a [AddedSkill],
    }

    serde_json::to_string_pretty(&SkillsManifest {
        generated_by: "history-to-md",
        detected_technologies,
        added_skills,
    })
    .map_err(|error| format!("failed to serialize skills manifest: {error}"))
}

fn collect_file_paths(node: &TreeNode, files: &mut Vec<String>) {
    if node.is_dir {
        for child in &node.children {
            collect_file_paths(child, files);
        }
        return;
    }

    if !node.path.is_empty() {
        files.push(node.path.clone());
    }
}

fn push_detected_technology(
    detected: &mut Vec<DetectedTechnology>,
    id: &str,
    name: &str,
    evidence: Vec<Option<String>>,
) {
    let evidence = evidence.into_iter().flatten().collect::<Vec<_>>();
    if evidence.is_empty() {
        return;
    }

    detected.push(DetectedTechnology {
        id: id.to_string(),
        name: name.to_string(),
        evidence,
    });
}

fn find_exact_path(file_set: &HashSet<&str>, files: &[String], exact_path: &str) -> Option<String> {
    if file_set.contains(exact_path) {
        return Some(format!("Found `{exact_path}`"));
    }

    files
        .iter()
        .find(|path| path.ends_with(&format!("/{exact_path}")))
        .map(|path| format!("Found `{path}`"))
}

fn find_prefix_path(files: &[String], prefix: &str) -> Option<String> {
    files
        .iter()
        .find(|path| path.starts_with(prefix))
        .map(|path| format!("Found `{path}`"))
}

fn find_suffix_path(files: &[String], suffix: &str) -> Option<String> {
    files
        .iter()
        .find(|path| path.ends_with(suffix))
        .map(|path| format!("Found `{path}`"))
}

fn find_path_with_extension(files: &[String], extension: &str) -> Option<String> {
    let expected = format!(".{extension}");
    files
        .iter()
        .find(|path| path.ends_with(&expected))
        .map(|path| format!("Found `{path}`"))
}

fn file_contains_any(
    repo_path: &Path,
    relative_path: &str,
    needles: &[&str],
) -> Result<Option<String>, String> {
    let file_path = repo_path.join(relative_path);
    if !file_path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&file_path).map_err(|error| {
        format!(
            "failed to read technology marker file {}: {error}",
            file_path.display()
        )
    })?;

    Ok(needles
        .iter()
        .find(|needle| contents.contains(**needle))
        .map(|needle| format!("Found `{relative_path}` containing `{needle}`")))
}

fn copy_path_recursively(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::metadata(source)
        .map_err(|error| format!("failed to read skill source {}: {error}", source.display()))?;

    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(|error| {
            format!(
                "failed to create skill directory {}: {error}",
                destination.display()
            )
        })?;
        for entry in fs::read_dir(source).map_err(|error| {
            format!(
                "failed to read skill directory {}: {error}",
                source.display()
            )
        })? {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to read entry under skill directory {}: {error}",
                    source.display()
                )
            })?;
            copy_path_recursively(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create parent directory for {}: {error}",
                destination.display()
            )
        })?;
    }

    fs::copy(source, destination).map_err(|error| {
        format!(
            "failed to copy skill {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn relative_href(output_dir: &Path, target_path: &Path) -> Option<String> {
    target_path
        .strip_prefix(output_dir)
        .ok()
        .map(path_to_string)
}

fn display_skill_location(path: &Path) -> String {
    path.display().to_string()
}

fn preferred_skill_link_target(path: &Path, is_directory: bool) -> PathBuf {
    if !is_directory {
        return path.to_path_buf();
    }

    let skill_markdown = path.join("SKILL.md");
    if skill_markdown.exists() {
        skill_markdown
    } else {
        path.to_path_buf()
    }
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

        fs::write(&destination, render_file_summary(report, file))
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

        fs::write(&destination, render_directory_summary(report, directory)).map_err(|error| {
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
    writeln!(&mut markdown, "- Web viewer: [index.html](./index.html)").unwrap();
    writeln!(
        &mut markdown,
        "- Agent profile: {}",
        report.agent_profile.display_name()
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
        report.file_histories.len()
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- Folders with history: {}",
        report.directory_histories.len().saturating_sub(1)
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

fn render_file_summary(report: &RepoReport, file: &PathHistory) -> String {
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

fn render_directory_summary(report: &RepoReport, directory: &PathHistory) -> String {
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

fn render_html_viewer(report: &RepoReport) -> Result<String, String> {
    let html_data = HtmlReportData {
        repo_name: report.repo_name.clone(),
        agent_profile: report.agent_profile.display_name().to_string(),
        scanned_commits: report.scanned_commits,
        changed_files: report.file_histories.len(),
        changed_directories: report.directory_histories.len().saturating_sub(1),
        detected_technologies: report.detected_technologies.clone(),
        added_skills: report.added_skills.clone(),
        skills_manifest_href: report.skills_manifest_href.clone(),
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
        "<div class=\"shell\"><aside class=\"sidebar\"><div class=\"sidebar-header\"><p class=\"eyebrow\">History to MD</p><h1>{}</h1><p class=\"meta\">{} commits scanned • {} files with history • {} folders with history</p><p class=\"meta\">Markdown profile: {}</p><p class=\"meta\">{} technologies detected • {} skills added</p></div><nav class=\"tree\">{}</nav></aside><main class=\"content\"><div class=\"panel\" id=\"node-details\"></div></main></div>",
        escape_html(&report.repo_name),
        report.scanned_commits,
        report.file_histories.len(),
        report.directory_histories.len().saturating_sub(1),
        escape_html(report.agent_profile.display_name()),
        report.detected_technologies.len(),
        report.added_skills.len(),
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

    if node.path.is_empty() {
        if let Some(manifest_href) = report.skills_manifest_href.as_ref() {
            links.push(ReportLink {
                label: "Skills manifest".to_string(),
                href: manifest_href.clone(),
            });
        }
    }

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

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
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

function renderTechnologyList() {
  if (!data.detected_technologies.length) {
    return `<li class="empty-state">No technologies detected.</li>`;
  }

  return data.detected_technologies
    .map(
      (technology) =>
        `<li><strong>${escapeHtml(technology.name)}</strong><br><span class="commit-meta">${escapeHtml(technology.evidence.join(", "))}</span></li>`
    )
    .join("");
}

function renderSkillsList() {
  if (!data.added_skills.length) {
    return `<li class="empty-state">No matching skills were added.</li>`;
  }

  const manifestLink = data.skills_manifest_href
    ? `<li><a href="${encodeURI(data.skills_manifest_href)}" target="_blank" rel="noreferrer">Skills manifest</a></li>`
    : "";

  return `
    ${data.added_skills
      .map((skill) => {
        const location = skill.href
          ? `<a href="${encodeURI(skill.href)}" target="_blank" rel="noreferrer">${escapeHtml(skill.location)}</a>`
          : `<code>${escapeHtml(skill.location)}</code>`;
        return `<li><strong>${escapeHtml(skill.title)}</strong><br><span class="commit-meta">${escapeHtml(
          skill.description
        )} Matched: ${escapeHtml(skill.matched_technologies.join(", "))}. Installed at ${location}.</span></li>`;
      })
      .join("")}
    ${manifestLink}
  `;
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

  const rootSections =
    path === ""
      ? `
        <section class="list-card" style="margin-top: 16px;">
          <h3 class="section-title">Detected Technologies</h3>
          <ul class="link-list">${renderTechnologyList()}</ul>
        </section>
        <section class="list-card" style="margin-top: 16px;">
          <h3 class="section-title">Skills From Database</h3>
          <ul class="link-list">${renderSkillsList()}</ul>
        </section>
      `
      : "";

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
    ${rootSections}
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
        AddedSkill, AgentProfile, CommitMeta, Config, DetectedTechnology, FileCommit,
        HistoryAccumulator, PathHistory, RepoReport, SkillMatchMode, SkillsDatabaseConfig,
        SkillsDatabaseEntry, TreeNode, add_skills_from_database, ancestor_directories,
        build_repo_tree, collect_history, detect_technologies, directory_markdown_path,
        directory_summary_link, markdown_path, matched_technology_ids, parse_commit_meta,
        parse_numstat_line, preferred_skill_link_target, relative_href, relevant_report_links,
        render_file_summary, render_html_viewer, render_summary, serialize_for_html,
        specific_directory_chain, summary_link, top_authors, write_report, yaml_string,
    };
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn parses_binary_numstat_lines_as_zero_churn() {
        let change = parse_numstat_line("-\t-\tassets/logo.png").expect("numstat should parse");
        assert_eq!(change.0, 0);
        assert_eq!(change.1, 0);
        assert_eq!(change.2, "assets/logo.png");
    }

    #[test]
    fn rejects_commit_metadata_without_subject() {
        let error = parse_commit_meta("abc123\u{1f}2026-04-16\u{1f}Jane Doe")
            .expect_err("commit metadata should fail without subject");
        assert_eq!(error, "missing commit subject in git log output");
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

    #[test]
    fn config_accepts_agent_profile_flag() {
        let repo_path = unique_temp_path("history-to-md-config-test");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");

        let args = vec![
            "history-to-md".to_string(),
            "--agent".to_string(),
            "codex".to_string(),
            repo_path.display().to_string(),
        ];
        let config = Config::from_args(&args).expect("config should parse");

        assert_eq!(config.repo_path, repo_path);
        assert_eq!(config.output_dir, repo_path.join("history-md"));
        assert_eq!(config.agent_profile, AgentProfile::Codex);
        assert!(config.skills_database.is_none());

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn config_accepts_skills_database_flags() {
        let repo_path = unique_temp_path("history-to-md-skills-config-test");
        let database_root = unique_temp_path("history-to-md-skills-db");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");
        fs::create_dir_all(&database_root).expect("skills db dir should be created");
        fs::write(database_root.join("skills.json"), "{\"skills\":[]}")
            .expect("skills db file should be written");

        let args = vec![
            "history-to-md".to_string(),
            "--skills-db".to_string(),
            database_root.join("skills.json").display().to_string(),
            "--skills-dir".to_string(),
            repo_path.join(".codex/skills").display().to_string(),
            repo_path.display().to_string(),
        ];
        let config = Config::from_args(&args).expect("config should parse");
        let skills_database = config
            .skills_database
            .expect("skills database config should be set");

        assert_eq!(
            skills_database.database_path,
            database_root.join("skills.json")
        );
        assert_eq!(skills_database.install_dir, repo_path.join(".codex/skills"));

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
        fs::remove_dir_all(&database_root).expect("skills db dir should be cleaned up");
    }

    #[test]
    fn config_rejects_unknown_option() {
        let repo_path = unique_temp_path("history-to-md-config-unknown-option");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");

        let args = vec![
            "history-to-md".to_string(),
            "--wat".to_string(),
            repo_path.display().to_string(),
        ];
        let error = match Config::from_args(&args) {
            Ok(_) => panic!("config should reject unknown options"),
            Err(error) => error,
        };

        assert!(error.contains("unknown option: --wat"));
        assert!(error.contains("usage: history-to-md"));

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn config_requires_skills_database_when_skills_dir_is_provided() {
        let repo_path = unique_temp_path("history-to-md-config-skills-dir");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");

        let args = vec![
            "history-to-md".to_string(),
            "--skills-dir".to_string(),
            repo_path.join(".codex/skills").display().to_string(),
            repo_path.display().to_string(),
        ];
        let error = match Config::from_args(&args) {
            Ok(_) => panic!("config should reject orphan skills dir"),
            Err(error) => error,
        };

        assert!(error.contains("--skills-dir requires --skills-db"));

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn config_rejects_missing_skills_database_path() {
        let repo_path = unique_temp_path("history-to-md-config-missing-skills-db");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");
        let missing_database = repo_path.join("missing.json");

        let args = vec![
            "history-to-md".to_string(),
            "--skills-db".to_string(),
            missing_database.display().to_string(),
            repo_path.display().to_string(),
        ];
        let error = match Config::from_args(&args) {
            Ok(_) => panic!("config should reject missing database"),
            Err(error) => error,
        };

        assert!(
            error.contains(&format!(
                "skills database path does not exist: {}",
                missing_database.display()
            ))
        );

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn rejects_unknown_agent_profile() {
        let error = AgentProfile::parse("unknown").expect_err("agent parsing should fail");
        assert!(error.contains("supported agent profiles: generic, codex, claude, cursor, aider"));
    }

    #[test]
    fn detects_technologies_from_repository_tree() {
        let repo_path = unique_temp_path("history-to-md-tech-detect-test");
        fs::create_dir_all(repo_path.join("src")).expect("src dir should exist");
        fs::create_dir_all(repo_path.join("web")).expect("web dir should exist");
        fs::write(repo_path.join("Cargo.toml"), "[package]\nname = \"demo\"\n")
            .expect("cargo manifest should be written");
        fs::write(
            repo_path.join("package.json"),
            "{\n  \"dependencies\": { \"react\": \"18.0.0\" }\n}\n",
        )
        .expect("package json should be written");
        fs::write(repo_path.join("src/main.rs"), "fn main() {}\n")
            .expect("rust file should be written");
        fs::write(
            repo_path.join("web/app.tsx"),
            "export const App = () => null;\n",
        )
        .expect("tsx file should be written");
        fs::write(repo_path.join("Dockerfile"), "FROM rust:1.0\n")
            .expect("dockerfile should be written");

        let tree = build_repo_tree(&repo_path, &repo_path.join("history-md"))
            .expect("tree should build successfully");
        let technologies =
            detect_technologies(&repo_path, &tree).expect("technologies should detect");

        let names = technologies
            .iter()
            .map(|technology| technology.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"Rust"));
        assert!(names.contains(&"Node.js"));
        assert!(names.contains(&"TypeScript"));
        assert!(names.contains(&"React"));
        assert!(names.contains(&"Docker"));

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn detects_react_from_package_json_content_without_component_files() {
        let repo_path = unique_temp_path("history-to-md-react-detect-test");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");
        fs::write(
            repo_path.join("package.json"),
            "{\n  \"dependencies\": { \"react\": \"18.0.0\" }\n}\n",
        )
        .expect("package json should be written");

        let tree = build_repo_tree(&repo_path, &repo_path.join("history-md"))
            .expect("tree should build successfully");
        let technologies =
            detect_technologies(&repo_path, &tree).expect("technologies should detect");

        let react = technologies
            .iter()
            .find(|technology| technology.id == "react")
            .expect("react should be detected");
        assert_eq!(
            react.evidence,
            vec!["Found `package.json` containing `\"react\"`".to_string()]
        );
        assert_eq!(
            technologies
                .iter()
                .map(|technology| technology.name.as_str())
                .collect::<Vec<_>>(),
            vec!["JavaScript", "Node.js", "React"]
        );

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn matches_skill_technologies_for_any_and_all_modes() {
        let detected_one = HashSet::from(["rust"]);
        let detected_two = HashSet::from(["rust", "typescript"]);
        let any_skill = SkillsDatabaseEntry {
            id: "polyglot".to_string(),
            title: "Polyglot".to_string(),
            description: "Matches any configured technology.".to_string(),
            technologies: vec!["rust".to_string(), "typescript".to_string()],
            match_mode: SkillMatchMode::Any,
            source: Some("polyglot".to_string()),
            install_as: None,
        };
        let all_skill = SkillsDatabaseEntry {
            match_mode: SkillMatchMode::All,
            ..any_skill.clone()
        };

        assert_eq!(matched_technology_ids(&any_skill, &detected_one), vec!["rust"]);
        assert!(matched_technology_ids(&all_skill, &detected_one).is_empty());
        assert_eq!(
            matched_technology_ids(&all_skill, &detected_two),
            vec!["rust", "typescript"]
        );
    }

    #[test]
    fn adds_matching_skills_from_database() {
        let repo_path = unique_temp_path("history-to-md-skills-match-test");
        let output_dir = repo_path.join("history-md");
        let db_root = unique_temp_path("history-to-md-skills-db-match");
        let install_dir = output_dir.join("skills");
        fs::create_dir_all(db_root.join("rust-review")).expect("skill dir should exist");
        fs::create_dir_all(db_root.join("frontend-review")).expect("skill dir should exist");
        fs::write(
            db_root.join("rust-review/SKILL.md"),
            "# Rust Review\nUse for Rust repos.\n",
        )
        .expect("rust skill should be written");
        fs::write(
            db_root.join("frontend-review/SKILL.md"),
            "# Frontend Review\nUse for TS and React repos.\n",
        )
        .expect("frontend skill should be written");
        fs::write(
            db_root.join("skills.json"),
            r#"{
  "skills": [
    {
      "id": "rust-review",
      "title": "Rust Review",
      "description": "Rust-oriented review heuristics.",
      "technologies": ["rust"],
      "source": "rust-review"
    },
    {
      "id": "frontend-review",
      "title": "Frontend Review",
      "description": "Frontend heuristics for React and TypeScript.",
      "technologies": ["react", "typescript"],
      "match_mode": "all",
      "source": "frontend-review"
    },
    {
      "id": "go-review",
      "title": "Go Review",
      "description": "Go heuristics.",
      "technologies": ["go"],
      "source": "go-review"
    }
  ]
}"#,
        )
        .expect("skills db should be written");

        let skills = add_skills_from_database(
            &SkillsDatabaseConfig {
                database_path: db_root.join("skills.json"),
                install_dir: install_dir.clone(),
            },
            &output_dir,
            &[
                DetectedTechnology {
                    id: "react".to_string(),
                    name: "React".to_string(),
                    evidence: vec!["Found `package.json` containing `\"react\"`".to_string()],
                },
                DetectedTechnology {
                    id: "rust".to_string(),
                    name: "Rust".to_string(),
                    evidence: vec!["Found `Cargo.toml`".to_string()],
                },
                DetectedTechnology {
                    id: "typescript".to_string(),
                    name: "TypeScript".to_string(),
                    evidence: vec!["Found `web/app.tsx`".to_string()],
                },
            ],
        )
        .expect("skills should be added");

        assert_eq!(skills.added_skills.len(), 2);
        assert!(install_dir.join("rust-review/SKILL.md").exists());
        assert!(install_dir.join("frontend-review/SKILL.md").exists());
        assert!(install_dir.join("manifest.json").exists());

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
        fs::remove_dir_all(&db_root).expect("skills db dir should be cleaned up");
    }

    #[test]
    fn skips_unmatched_skills_without_a_source_path() {
        let repo_path = unique_temp_path("history-to-md-skills-unmatched-test");
        let output_dir = repo_path.join("history-md");
        let db_root = unique_temp_path("history-to-md-skills-db-unmatched");
        let install_dir = output_dir.join("skills");
        fs::create_dir_all(db_root.join("rust-review")).expect("skill dir should exist");
        fs::write(
            db_root.join("rust-review/SKILL.md"),
            "# Rust Review\nUse for Rust repos.\n",
        )
        .expect("rust skill should be written");
        fs::write(
            db_root.join("skills.json"),
            r#"{
  "skills": [
    {
      "id": "rust-review",
      "title": "Rust Review",
      "description": "Rust-oriented review heuristics.",
      "technologies": ["rust"],
      "source": "rust-review"
    },
    {
      "id": "go-review",
      "title": "Go Review",
      "description": "Go heuristics.",
      "technologies": ["go"]
    }
  ]
}"#,
        )
        .expect("skills db should be written");

        let skills = add_skills_from_database(
            &SkillsDatabaseConfig {
                database_path: db_root.join("skills.json"),
                install_dir: install_dir.clone(),
            },
            &output_dir,
            &[DetectedTechnology {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                evidence: vec!["Found `Cargo.toml`".to_string()],
            }],
        )
        .expect("matched skills should be added");

        assert_eq!(skills.added_skills.len(), 1);
        assert_eq!(skills.added_skills[0].id, "rust-review");
        assert!(install_dir.join("rust-review/SKILL.md").exists());

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
        fs::remove_dir_all(&db_root).expect("skills db dir should be cleaned up");
    }

    #[test]
    fn falls_back_to_skill_id_when_install_as_is_blank() {
        let repo_path = unique_temp_path("history-to-md-skills-install-name-test");
        let output_dir = repo_path.join("history-md");
        let db_root = unique_temp_path("history-to-md-skills-db-install-name");
        let install_dir = output_dir.join("skills");
        fs::create_dir_all(&db_root).expect("skills db dir should exist");
        fs::write(
            db_root.join("rust-review.md"),
            "# Rust Review\nUse for Rust repos.\n",
        )
        .expect("skill file should be written");
        fs::write(
            db_root.join("skills.json"),
            r#"{
  "skills": [
    {
      "id": "rust-review",
      "title": "Rust Review",
      "description": "Rust-oriented review heuristics.",
      "technologies": ["rust"],
      "source": "rust-review.md",
      "install_as": "   "
    }
  ]
}"#,
        )
        .expect("skills db should be written");

        let skills = add_skills_from_database(
            &SkillsDatabaseConfig {
                database_path: db_root.join("skills.json"),
                install_dir: install_dir.clone(),
            },
            &output_dir,
            &[DetectedTechnology {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                evidence: vec!["Found `Cargo.toml`".to_string()],
            }],
        )
        .expect("skill should be added");

        assert_eq!(skills.added_skills.len(), 1);
        assert_eq!(skills.added_skills[0].location, install_dir.join("rust-review").display().to_string());
        assert_eq!(skills.added_skills[0].href.as_deref(), Some("skills/rust-review"));
        assert!(install_dir.join("rust-review").is_file());

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
        fs::remove_dir_all(&db_root).expect("skills db dir should be cleaned up");
    }

    #[test]
    fn summary_includes_agent_frontmatter() {
        let report = RepoReport {
            repo_name: "demo".to_string(),
            scanned_commits: 12,
            file_histories: HashMap::new(),
            directory_histories: HashMap::new(),
            tree: TreeNode {
                path: String::new(),
                name: "demo".to_string(),
                is_dir: true,
                children: Vec::new(),
            },
            agent_profile: AgentProfile::Codex,
            detected_technologies: vec![DetectedTechnology {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                evidence: vec!["Found `Cargo.toml`".to_string()],
            }],
            added_skills: vec![AddedSkill {
                id: "rust-review".to_string(),
                title: "Rust Review".to_string(),
                description: "Rust-oriented review heuristics.".to_string(),
                matched_technologies: vec!["Rust".to_string()],
                location: "/tmp/demo/history-md/skills/rust-review/SKILL.md".to_string(),
                href: Some("skills/rust-review/SKILL.md".to_string()),
            }],
            skills_manifest_href: Some("skills/manifest.json".to_string()),
        };

        let markdown = render_summary(&report);
        assert!(markdown.contains("agent_profile: codex"));
        assert!(markdown.contains("## Agent Format"));
        assert!(markdown.contains("- Target agent: Codex"));
        assert!(markdown.contains("## Technology detection"));
        assert!(markdown.contains("## Skills from database"));
        assert!(markdown.contains("Rust Review"));
    }

    #[test]
    fn summary_renders_empty_detection_and_skills_states() {
        let mut report = sample_report();
        report.added_skills.clear();
        report.skills_manifest_href = None;

        let markdown = render_summary(&report);
        assert!(markdown.contains("- Detected technologies: none"));
        assert!(markdown.contains("- Added skills: none"));
        assert!(markdown.contains("- No technologies detected."));
        assert!(markdown.contains("- No matching skills were added from a skills database."));
    }

    #[test]
    fn repo_tree_skips_git_and_generated_output() {
        let repo_path = unique_temp_path("history-to-md-tree-test");
        fs::create_dir_all(repo_path.join(".git")).expect("git dir should exist");
        fs::create_dir_all(repo_path.join("src")).expect("src dir should exist");
        fs::create_dir_all(repo_path.join("history-md")).expect("output dir should exist");
        fs::write(repo_path.join("src/main.rs"), "fn main() {}\n")
            .expect("source file should exist");
        fs::write(repo_path.join("history-md/SUMMARY.md"), "# generated\n")
            .expect("generated file should exist");

        let tree = build_repo_tree(&repo_path, &repo_path.join("history-md"))
            .expect("tree should build successfully");

        let child_names: Vec<_> = tree
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert!(child_names.contains(&"src"));
        assert!(!child_names.contains(&".git"));
        assert!(!child_names.contains(&"history-md"));

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn repo_tree_sorts_directories_before_files_case_insensitively() {
        let repo_path = unique_temp_path("history-to-md-tree-sort-test");
        fs::create_dir_all(repo_path.join("Zoo")).expect("Zoo dir should exist");
        fs::create_dir_all(repo_path.join("alpha")).expect("alpha dir should exist");
        fs::write(repo_path.join("beta.txt"), "beta\n").expect("beta file should exist");
        fs::write(repo_path.join("Gamma.txt"), "gamma\n").expect("gamma file should exist");

        let tree = build_repo_tree(&repo_path, &repo_path.join("history-md"))
            .expect("tree should build successfully");
        let child_names = tree
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(child_names, vec!["alpha", "Zoo", "beta.txt", "Gamma.txt"]);

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn html_serialization_escapes_script_terminators() {
        let serialized =
            serialize_for_html(&vec!["</script>".to_string()]).expect("json should serialize");
        assert!(serialized.contains("<\\/script>"));
    }

    #[test]
    fn preferred_skill_link_target_uses_skill_markdown_for_directories() {
        let skill_dir = unique_temp_path("history-to-md-skill-link-target");
        fs::create_dir_all(&skill_dir).expect("skill dir should exist");
        fs::write(skill_dir.join("SKILL.md"), "# Skill\n").expect("skill markdown should exist");

        assert_eq!(
            preferred_skill_link_target(&skill_dir, true),
            skill_dir.join("SKILL.md")
        );
        assert_eq!(
            preferred_skill_link_target(&skill_dir.join("SKILL.md"), false),
            skill_dir.join("SKILL.md")
        );

        fs::remove_dir_all(&skill_dir).expect("temp skill dir should be cleaned up");
    }

    #[test]
    fn relative_href_only_links_paths_inside_output_directory() {
        let output_dir = Path::new("/tmp/history-md-output");
        assert_eq!(
            relative_href(output_dir, &output_dir.join("skills/manifest.json")).as_deref(),
            Some("skills/manifest.json")
        );
        assert_eq!(relative_href(output_dir, Path::new("/tmp/elsewhere/file.txt")), None);
    }

    #[test]
    fn file_summary_limits_commit_preview() {
        let report = sample_report();
        let commits = (0..15)
            .map(|index| FileCommit {
                commit: CommitMeta {
                    hash: format!("abcdef{index:02}"),
                    date: "2026-04-16".to_string(),
                    author: "Jane Doe".to_string(),
                    subject: format!("Commit {index:02}"),
                },
                added: index + 1,
                deleted: index,
            })
            .collect::<Vec<_>>();
        let file = PathHistory {
            path: "src/main.rs".to_string(),
            commit_count: commits.len() as u64,
            total_added: commits.iter().map(|commit| commit.added).sum(),
            total_deleted: commits.iter().map(|commit| commit.deleted).sum(),
            authors: HashMap::from([("Jane Doe".to_string(), commits.len() as u64)]),
            commits,
        };

        let markdown = render_file_summary(&report, &file);
        assert!(markdown.contains("Commit 00"));
        assert!(markdown.contains("Commit 11"));
        assert!(!markdown.contains("Commit 12"));
        assert!(!markdown.contains("Commit 14"));
    }

    #[test]
    fn top_authors_orders_ties_alphabetically() {
        let history = PathHistory {
            path: "src/main.rs".to_string(),
            commit_count: 4,
            total_added: 10,
            total_deleted: 2,
            authors: HashMap::from([
                ("Zoe".to_string(), 2),
                ("Amy".to_string(), 2),
                ("Bob".to_string(), 1),
            ]),
            commits: Vec::new(),
        };

        assert_eq!(top_authors(&history, 2), "Amy (2), Zoe (2)");
        assert_eq!(top_authors(&history, 5), "Amy (2), Zoe (2), Bob (1)");
    }

    #[test]
    fn relevant_report_links_include_manifest_and_parent_folder_history() {
        let report = sample_report();
        let root_links = relevant_report_links(&report.tree, &report);
        let file_links = relevant_report_links(&report.tree.children[0].children[0], &report);

        assert_eq!(root_links[0].label, "Skills manifest");
        assert_eq!(root_links[0].href, "skills/manifest.json");
        assert_eq!(
            file_links
                .iter()
                .map(|link| (link.label.as_str(), link.href.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("File history", "files/src/main.rs.md"),
                ("Folder history: src", "dirs/src/INDEX.md"),
                ("Repository summary", "SUMMARY.md"),
            ]
        );
    }

    #[test]
    fn yaml_strings_escape_single_quotes() {
        assert_eq!(yaml_string("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn collect_history_rejects_non_git_directories() {
        let repo_path = unique_temp_path("history-to-md-not-a-repo");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");

        let error = collect_history(&repo_path).expect_err("history collection should fail");
        assert!(error.contains("not a git repository"));

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    }

    #[test]
    fn generates_reports_for_a_real_git_repository() {
        let repo_path = unique_temp_path("history-to-md-e2e-test");
        fs::create_dir_all(&repo_path).expect("temp repo path should be created");
        init_git_repository(&repo_path);

        write_file(&repo_path.join("README.md"), "# demo\n");
        git_commit(&repo_path, "Add readme", "Jane Doe", "jane@example.com");

        write_file(
            &repo_path.join("src/main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        );
        write_file(&repo_path.join("README.md"), "# demo\n\nupdated\n");
        git_commit(&repo_path, "Add CLI", "John Roe", "john@example.com");

        let history = collect_history(&repo_path).expect("history should collect");
        assert_eq!(history.scanned_commits, 2);
        assert!(history.file_histories.contains_key("README.md"));
        assert!(history.file_histories.contains_key("src/main.rs"));
        assert!(history.directory_histories.contains_key("src"));

        let output_dir = repo_path.join("history-md");
        let db_root = unique_temp_path("history-to-md-e2e-skills-db");
        fs::create_dir_all(db_root.join("rust-review")).expect("skill dir should be created");
        fs::write(
            db_root.join("rust-review/SKILL.md"),
            "# Rust Review\nUse for Rust repos.\n",
        )
        .expect("skill file should be written");
        fs::write(
            db_root.join("skills.json"),
            r#"{
  "skills": [
    {
      "id": "rust-review",
      "title": "Rust Review",
      "description": "Rust-oriented review heuristics.",
      "technologies": ["rust"],
      "source": "rust-review"
    }
  ]
}"#,
        )
        .expect("skills db should be written");
        let skills_result = add_skills_from_database(
            &SkillsDatabaseConfig {
                database_path: db_root.join("skills.json"),
                install_dir: output_dir.join("skills"),
            },
            &output_dir,
            &[DetectedTechnology {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                evidence: vec!["Found `src/main.rs`".to_string()],
            }],
        )
        .expect("skills should be added");
        let report = RepoReport {
            repo_name: "demo".to_string(),
            scanned_commits: history.scanned_commits,
            file_histories: history.file_histories,
            directory_histories: history.directory_histories,
            tree: build_repo_tree(&repo_path, &output_dir).expect("tree should build"),
            agent_profile: AgentProfile::Codex,
            detected_technologies: vec![DetectedTechnology {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                evidence: vec!["Found `src/main.rs`".to_string()],
            }],
            added_skills: skills_result.added_skills,
            skills_manifest_href: skills_result.skills_manifest_href,
        };

        write_report(&output_dir, &report).expect("report should be written");

        let summary =
            fs::read_to_string(output_dir.join("SUMMARY.md")).expect("summary should be readable");
        let file_summary = fs::read_to_string(output_dir.join("files/src/main.rs.md"))
            .expect("file summary should be readable");
        let directory_summary = fs::read_to_string(output_dir.join("dirs/src/INDEX.md"))
            .expect("directory summary should be readable");
        let html =
            fs::read_to_string(output_dir.join("index.html")).expect("html should be readable");

        assert!(summary.contains("agent_profile: codex"));
        assert!(summary.contains("- Agent profile: Codex"));
        assert!(file_summary.contains("# src/main.rs"));
        assert!(file_summary.contains("## Agent Format"));
        assert!(file_summary.contains("Add CLI by John Roe"));
        assert!(directory_summary.contains("# Folder: src"));
        assert!(html.contains("Markdown profile: Codex"));
        assert!(html.contains("technologies detected"));
        assert!(html.contains("Repository summary"));
        assert!(
            render_html_viewer(&report)
                .expect("html should render")
                .contains("node-details")
        );
        assert!(output_dir.join("skills/rust-review/SKILL.md").exists());
        assert!(output_dir.join("skills/manifest.json").exists());

        fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
        fs::remove_dir_all(&db_root).expect("skills db dir should be cleaned up");
    }

    fn unique_temp_path(prefix: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nonce}"))
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should be created");
        }
        fs::write(path, contents).expect("file should be written");
    }

    fn init_git_repository(repo_path: &Path) {
        run_git(repo_path, &["init"], &[]);
        run_git(repo_path, &["config", "user.name", "Test User"], &[]);
        run_git(
            repo_path,
            &["config", "user.email", "test@example.com"],
            &[],
        );
    }

    fn git_commit(repo_path: &Path, message: &str, author_name: &str, author_email: &str) {
        run_git(repo_path, &["add", "."], &[]);
        run_git(
            repo_path,
            &["commit", "-m", message],
            &[
                ("GIT_AUTHOR_NAME", author_name),
                ("GIT_AUTHOR_EMAIL", author_email),
                ("GIT_COMMITTER_NAME", author_name),
                ("GIT_COMMITTER_EMAIL", author_email),
            ],
        );
    }

    fn run_git(repo_path: &Path, args: &[&str], envs: &[(&str, &str)]) {
        let mut command = Command::new("git");
        command.arg("-C").arg(repo_path).args(args);
        for (key, value) in envs {
            command.env(key, value);
        }

        let output = command.output().expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn sample_report() -> RepoReport {
        let mut directory_histories = HashMap::new();
        directory_histories.insert(
            String::new(),
            sample_history("", &[("root0001", "Jane Doe", "Initial import", 5, 1)]),
        );
        directory_histories.insert(
            "src".to_string(),
            sample_history("src", &[("src00001", "Jane Doe", "Touch src", 3, 1)]),
        );

        let mut file_histories = HashMap::new();
        file_histories.insert(
            "src/main.rs".to_string(),
            sample_history(
                "src/main.rs",
                &[("file0001", "Jane Doe", "Touch src", 3, 1)],
            ),
        );

        RepoReport {
            repo_name: "demo".to_string(),
            scanned_commits: 3,
            file_histories,
            directory_histories,
            tree: TreeNode {
                path: String::new(),
                name: "demo".to_string(),
                is_dir: true,
                children: vec![TreeNode {
                    path: "src".to_string(),
                    name: "src".to_string(),
                    is_dir: true,
                    children: vec![TreeNode {
                        path: "src/main.rs".to_string(),
                        name: "main.rs".to_string(),
                        is_dir: false,
                        children: Vec::new(),
                    }],
                }],
            },
            agent_profile: AgentProfile::Codex,
            detected_technologies: Vec::new(),
            added_skills: vec![AddedSkill {
                id: "rust-review".to_string(),
                title: "Rust Review".to_string(),
                description: "Rust-oriented review heuristics.".to_string(),
                matched_technologies: vec!["Rust".to_string()],
                location: "/tmp/demo/history-md/skills/rust-review/SKILL.md".to_string(),
                href: Some("skills/rust-review/SKILL.md".to_string()),
            }],
            skills_manifest_href: Some("skills/manifest.json".to_string()),
        }
    }

    fn sample_history(path: &str, commits: &[(&str, &str, &str, u64, u64)]) -> PathHistory {
        let mut history = HistoryAccumulator::new(path.to_string());
        for (hash, author, subject, added, deleted) in commits {
            history.record_change(
                &CommitMeta {
                    hash: (*hash).to_string(),
                    date: "2026-04-16".to_string(),
                    author: (*author).to_string(),
                    subject: (*subject).to_string(),
                },
                *added,
                *deleted,
            );
        }
        history.into_history()
    }
}
