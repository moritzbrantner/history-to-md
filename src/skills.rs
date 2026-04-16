use crate::cli::SkillsDatabaseConfig;
use crate::error::{AppError, AppResult};
use crate::model::{AddedSkill, DetectedTechnology, SkillsIntegration};
use crate::tree::path_to_string;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct SkillsDatabase {
    skills: Vec<SkillsDatabaseEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct SkillsDatabaseEntry {
    pub id: String,
    pub title: String,
    pub description: String,
    pub technologies: Vec<String>,
    #[serde(default)]
    pub match_mode: SkillMatchMode,
    pub source: Option<String>,
    pub install_as: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SkillMatchMode {
    #[default]
    Any,
    All,
}

pub fn add_skills_from_database(
    config: &SkillsDatabaseConfig,
    output_dir: &Path,
    detected_technologies: &[DetectedTechnology],
) -> AppResult<SkillsIntegration> {
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
        AppError::io(
            format!(
                "failed to create skills install directory {}",
                config.install_dir.display()
            ),
            error,
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
            AppError::message(format!(
                "matched skill '{}' is missing a source path in {}",
                skill.id,
                config.database_path.display()
            ))
        })?;
        let source_path = database_root.join(source);
        let source_metadata = fs::metadata(&source_path).map_err(|error| {
            AppError::io(
                format!("failed to inspect skill source {}", source_path.display()),
                error,
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

    added_skills.sort_by(|left, right| left.id.cmp(&right.id));

    let skills_manifest_href = if !added_skills.is_empty() {
        let manifest_path = config.install_dir.join("manifest.json");
        fs::write(
            &manifest_path,
            render_skills_manifest(detected_technologies, &added_skills)?,
        )
        .map_err(|error| {
            AppError::io(
                format!(
                    "failed to write skills manifest {}",
                    manifest_path.display()
                ),
                error,
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

fn load_skills_database(database_path: &Path) -> AppResult<SkillsDatabase> {
    let contents = fs::read_to_string(database_path).map_err(|error| {
        AppError::io(
            format!("failed to read skills database {}", database_path.display()),
            error,
        )
    })?;

    serde_json::from_str(&contents).map_err(|error| {
        AppError::json(
            format!(
                "failed to parse skills database {}",
                database_path.display()
            ),
            error,
        )
    })
}

pub fn matched_technology_ids<'a>(
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
) -> AppResult<String> {
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
    .map_err(|error| AppError::json("failed to serialize skills manifest", error))
}

fn copy_path_recursively(source: &Path, destination: &Path) -> AppResult<()> {
    let metadata = fs::metadata(source).map_err(|error| {
        AppError::io(
            format!("failed to read skill source {}", source.display()),
            error,
        )
    })?;

    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(|error| {
            AppError::io(
                format!("failed to create skill directory {}", destination.display()),
                error,
            )
        })?;
        for entry in fs::read_dir(source).map_err(|error| {
            AppError::io(
                format!("failed to read skill directory {}", source.display()),
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                AppError::io(
                    format!(
                        "failed to read entry under skill directory {}",
                        source.display()
                    ),
                    error,
                )
            })?;
            copy_path_recursively(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::io(
                format!(
                    "failed to create parent directory for {}",
                    destination.display()
                ),
                error,
            )
        })?;
    }

    fs::copy(source, destination).map_err(|error| {
        AppError::io(
            format!(
                "failed to copy skill {} to {}",
                source.display(),
                destination.display()
            ),
            error,
        )
    })?;
    Ok(())
}

pub fn relative_href(output_dir: &Path, target_path: &Path) -> Option<String> {
    target_path
        .strip_prefix(output_dir)
        .ok()
        .map(path_to_string)
}

fn display_skill_location(path: &Path) -> String {
    path.display().to_string()
}

pub fn preferred_skill_link_target(path: &Path, is_directory: bool) -> PathBuf {
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
