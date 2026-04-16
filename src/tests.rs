use crate::cli::{Config, SkillsDatabaseConfig};
use crate::git_history::{
    ancestor_directories, collect_history, parse_commit_meta, parse_numstat_line,
};
use crate::model::{
    AddedSkill, AgentProfile, CommitMeta, FileCommit, HistoryAccumulator, OutputFormats,
    PathHistory, RepoReport, TreeNode, directory_markdown_path, markdown_path,
};
use crate::render_html::{relevant_report_links, serialize_for_html};
use crate::render_markdown::{
    directory_summary_link, render_file_summary, render_summary, summary_link, top_authors,
    yaml_string,
};
use crate::skills::{
    SkillMatchMode, SkillsDatabaseEntry, add_skills_from_database, matched_technology_ids,
    preferred_skill_link_target, relative_href,
};
use crate::technology::detect_technologies;
use crate::tree::{build_repo_tree, specific_directory_chain};
use std::collections::{BTreeMap, HashSet};
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
    assert_eq!(
        error.to_string(),
        "missing commit subject in git log output"
    );
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
fn config_accepts_filter_and_format_flags() {
    let repo_path = unique_temp_path("history-to-md-config-filter-test");
    fs::create_dir_all(&repo_path).expect("temp repo path should be created");

    let args = vec![
        "history-to-md".to_string(),
        "--since".to_string(),
        "2026-01-01".to_string(),
        "--until".to_string(),
        "2026-12-31".to_string(),
        "--max-commits".to_string(),
        "5".to_string(),
        "--include".to_string(),
        "src/**".to_string(),
        "--exclude".to_string(),
        "target/**".to_string(),
        "--formats".to_string(),
        "md,json".to_string(),
        repo_path.display().to_string(),
    ];
    let config = Config::from_args(&args).expect("config should parse");

    assert_eq!(
        config.generation_options.since.as_deref(),
        Some("2026-01-01")
    );
    assert_eq!(
        config.generation_options.until.as_deref(),
        Some("2026-12-31")
    );
    assert_eq!(config.generation_options.max_commits, Some(5));
    assert!(config.output_formats.includes_markdown());
    assert!(!config.output_formats.includes_html());
    assert!(config.output_formats.includes_json());
    assert_eq!(
        config.generation_options.matcher.include_patterns(),
        &["src/**".to_string()]
    );
    assert_eq!(
        config.generation_options.matcher.exclude_patterns(),
        &["target/**".to_string()]
    );

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

    assert!(error.to_string().contains("unknown option: --wat"));
    assert!(error.to_string().contains("usage: history-to-md"));

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

    assert!(
        error
            .to_string()
            .contains("--skills-dir requires --skills-db")
    );

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

    assert!(error.to_string().contains(&format!(
        "skills database path does not exist: {}",
        missing_database.display()
    )));

    fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
}

#[test]
fn rejects_unknown_agent_profile() {
    let error = AgentProfile::parse("unknown").expect_err("agent parsing should fail");
    assert!(error.contains("supported agent profiles: generic, codex, claude, cursor, aider"));
}

#[test]
fn parses_output_formats() {
    assert_eq!(
        OutputFormats::parse("md,html,json")
            .expect("formats should parse")
            .to_labels(),
        vec!["md", "html", "json"]
    );
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

    let tree = build_repo_tree(
        &repo_path,
        &repo_path.join("history-md"),
        &Config::from_args(&["history-to-md".to_string(), repo_path.display().to_string()])
            .expect("config should parse")
            .generation_options,
    )
    .expect("tree should build successfully");
    let technologies = detect_technologies(&repo_path, &tree).expect("technologies should detect");

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

    let tree = build_repo_tree(
        &repo_path,
        &repo_path.join("history-md"),
        &Config::from_args(&["history-to-md".to_string(), repo_path.display().to_string()])
            .expect("config should parse")
            .generation_options,
    )
    .expect("tree should build successfully");
    let technologies = detect_technologies(&repo_path, &tree).expect("technologies should detect");

    let react = technologies
        .iter()
        .find(|technology| technology.id == "react")
        .expect("react should be detected");
    assert_eq!(
        react.evidence,
        vec!["Found `package.json` containing `\"react\"`".to_string()]
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

    assert_eq!(
        matched_technology_ids(&any_skill, &detected_one),
        vec!["rust"]
    );
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
            crate::model::DetectedTechnology {
                id: "react".to_string(),
                name: "React".to_string(),
                evidence: vec!["Found `package.json` containing `\"react\"`".to_string()],
            },
            crate::model::DetectedTechnology {
                id: "rust".to_string(),
                name: "Rust".to_string(),
                evidence: vec!["Found `Cargo.toml`".to_string()],
            },
            crate::model::DetectedTechnology {
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
        &[crate::model::DetectedTechnology {
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
        &[crate::model::DetectedTechnology {
            id: "rust".to_string(),
            name: "Rust".to_string(),
            evidence: vec!["Found `Cargo.toml`".to_string()],
        }],
    )
    .expect("skill should be added");

    assert_eq!(skills.added_skills.len(), 1);
    assert_eq!(
        skills.added_skills[0].location,
        install_dir.join("rust-review").display().to_string()
    );
    assert_eq!(
        skills.added_skills[0].href.as_deref(),
        Some("skills/rust-review")
    );
    assert!(install_dir.join("rust-review").is_file());

    fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    fs::remove_dir_all(&db_root).expect("skills db dir should be cleaned up");
}

#[test]
fn summary_includes_agent_frontmatter() {
    let report = sample_report();

    let markdown = render_summary(&report);
    assert!(markdown.contains("agent_profile: codex"));
    assert!(markdown.contains("## Agent Format"));
    assert!(markdown.contains("- Target agent: Codex"));
    assert!(markdown.contains("## Technology detection"));
    assert!(markdown.contains("## Skills from database"));
    assert!(markdown.contains("Rust Review"));
    assert!(markdown.contains("## Hotspots by churn"));
    assert!(markdown.contains("## Ownership concentration"));
    assert!(markdown.contains("## Recent activity"));
}

#[test]
fn summary_renders_empty_detection_and_skills_states() {
    let mut report = sample_report();
    report.detected_technologies.clear();
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
    fs::write(repo_path.join("src/main.rs"), "fn main() {}\n").expect("source file should exist");
    fs::write(repo_path.join("history-md/SUMMARY.md"), "# generated\n")
        .expect("generated file should exist");

    let tree = build_repo_tree(
        &repo_path,
        &repo_path.join("history-md"),
        &Config::from_args(&["history-to-md".to_string(), repo_path.display().to_string()])
            .expect("config should parse")
            .generation_options,
    )
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

    let tree = build_repo_tree(
        &repo_path,
        &repo_path.join("history-md"),
        &Config::from_args(&["history-to-md".to_string(), repo_path.display().to_string()])
            .expect("config should parse")
            .generation_options,
    )
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
fn tree_applies_include_and_exclude_filters() {
    let repo_path = unique_temp_path("history-to-md-tree-filter-test");
    fs::create_dir_all(repo_path.join("src")).expect("src dir should exist");
    fs::create_dir_all(repo_path.join("docs")).expect("docs dir should exist");
    fs::write(repo_path.join("src/main.rs"), "fn main() {}\n").expect("src file should exist");
    fs::write(repo_path.join("docs/readme.md"), "# docs\n").expect("docs file should exist");

    let args = vec![
        "history-to-md".to_string(),
        "--include".to_string(),
        "src/**".to_string(),
        "--exclude".to_string(),
        "src/generated/**".to_string(),
        repo_path.display().to_string(),
    ];
    let config = Config::from_args(&args).expect("config should parse");
    let tree = build_repo_tree(
        &repo_path,
        &repo_path.join("history-md"),
        &config.generation_options,
    )
    .expect("tree should build successfully");
    let child_names = tree
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(child_names, vec!["src"]);

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
    assert_eq!(
        relative_href(output_dir, Path::new("/tmp/elsewhere/file.txt")),
        None
    );
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
        authors: BTreeMap::from([("Jane Doe".to_string(), commits.len() as u64)]),
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
        authors: BTreeMap::from([
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

    let config = Config::from_args(&["history-to-md".to_string(), repo_path.display().to_string()])
        .expect("config should parse");
    let error = collect_history(&repo_path, &config.generation_options)
        .expect_err("history collection should fail");
    assert!(error.to_string().contains("not a git repository"));

    fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
}

#[test]
fn collect_history_respects_include_filters() {
    let repo_path = unique_temp_path("history-to-md-history-filter-test");
    fs::create_dir_all(&repo_path).expect("temp repo path should be created");
    init_git_repository(&repo_path);

    write_file(&repo_path.join("src/main.rs"), "fn main() {}\n");
    write_file(&repo_path.join("README.md"), "# demo\n");
    git_commit(
        &repo_path,
        "Add initial files",
        "Jane Doe",
        "jane@example.com",
    );

    let args = vec![
        "history-to-md".to_string(),
        "--include".to_string(),
        "src/**".to_string(),
        repo_path.display().to_string(),
    ];
    let config = Config::from_args(&args).expect("config should parse");
    let history =
        collect_history(&repo_path, &config.generation_options).expect("history should collect");

    assert!(history.file_histories.contains_key("src/main.rs"));
    assert!(!history.file_histories.contains_key("README.md"));

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

    let skills_db_root = unique_temp_path("history-to-md-e2e-skills-db");
    let skills_db_path = unique_skills_db(&skills_db_root);
    let args = vec![
        "history-to-md".to_string(),
        "--agent".to_string(),
        "codex".to_string(),
        "--skills-db".to_string(),
        skills_db_path.display().to_string(),
        repo_path.display().to_string(),
    ];
    let config = Config::from_args(&args).expect("config should parse");
    crate::run_with_config(config).expect("report should generate");

    let output_dir = repo_path.join("history-md");
    let summary =
        fs::read_to_string(output_dir.join("SUMMARY.md")).expect("summary should be readable");
    let file_summary = fs::read_to_string(output_dir.join("files/src/main.rs.md"))
        .expect("file summary should be readable");
    let directory_summary = fs::read_to_string(output_dir.join("dirs/src/INDEX.md"))
        .expect("directory summary should be readable");
    let html = fs::read_to_string(output_dir.join("index.html")).expect("html should be readable");
    let json = fs::read_to_string(output_dir.join("report.json")).expect("json should be readable");

    assert!(summary.contains("agent_profile: codex"));
    assert!(summary.contains("- Agent profile: Codex"));
    assert!(summary.contains("- Machine report: [report.json](./report.json)"));
    assert!(file_summary.contains("# src/main.rs"));
    assert!(file_summary.contains("## Agent Format"));
    assert!(file_summary.contains("Add CLI by John Roe"));
    assert!(directory_summary.contains("# Folder: src"));
    assert!(html.contains("Markdown profile: Codex"));
    assert!(html.contains("technologies detected"));
    assert!(html.contains("Repository summary"));
    assert!(json.contains("\"file_histories\""));
    assert!(output_dir.join("skills/rust-review/SKILL.md").exists());
    assert!(output_dir.join("skills/manifest.json").exists());

    fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
    fs::remove_dir_all(&skills_db_root).expect("temp skills db path should be cleaned up");
}

#[test]
fn supports_json_only_output() {
    let repo_path = unique_temp_path("history-to-md-json-only-test");
    fs::create_dir_all(&repo_path).expect("temp repo path should be created");
    init_git_repository(&repo_path);
    write_file(&repo_path.join("README.md"), "# demo\n");
    git_commit(&repo_path, "Add readme", "Jane Doe", "jane@example.com");

    let args = vec![
        "history-to-md".to_string(),
        "--formats".to_string(),
        "json".to_string(),
        repo_path.display().to_string(),
    ];
    let config = Config::from_args(&args).expect("config should parse");
    crate::run_with_config(config).expect("report should generate");

    let output_dir = repo_path.join("history-md");
    assert!(output_dir.join("report.json").exists());
    assert!(!output_dir.join("SUMMARY.md").exists());
    assert!(!output_dir.join("index.html").exists());

    fs::remove_dir_all(&repo_path).expect("temp repo path should be cleaned up");
}

fn unique_temp_path(prefix: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nonce}"))
}

fn unique_skills_db(db_root: &Path) -> std::path::PathBuf {
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
    db_root.join("skills.json")
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
    let mut directory_histories = BTreeMap::new();
    directory_histories.insert(
        String::new(),
        sample_history("", &[("root0001", "Jane Doe", "Initial import", 5, 1)]),
    );
    directory_histories.insert(
        "src".to_string(),
        sample_history("src", &[("src00001", "Jane Doe", "Touch src", 3, 1)]),
    );

    let mut file_histories = BTreeMap::new();
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
        detected_technologies: vec![crate::model::DetectedTechnology {
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
        include_patterns: Vec::new(),
        exclude_patterns: Vec::new(),
        output_formats: OutputFormats::default(),
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
