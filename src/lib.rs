pub mod cli;
pub mod error;
pub mod git_history;
pub mod model;
pub mod render_html;
pub mod render_markdown;
pub mod skills;
pub mod technology;
pub mod tree;

use crate::cli::Config;
use crate::error::{AppError, AppResult};
use crate::model::RepoReport;
use std::fs;

pub fn run() -> AppResult<()> {
    let config = Config::from_env()?;
    run_with_config(config)
}

pub fn run_with_config(config: Config) -> AppResult<()> {
    let history = git_history::collect_history(&config.repo_path, &config.generation_options)?;
    let tree = tree::build_repo_tree(
        &config.repo_path,
        &config.output_dir,
        &config.generation_options,
    )?;
    let detected_technologies = technology::detect_technologies(&config.repo_path, &tree)?;
    let skills_result = match config.skills_database.as_ref() {
        Some(skills_config) => skills::add_skills_from_database(
            skills_config,
            &config.output_dir,
            &detected_technologies,
        )?,
        None => Default::default(),
    };
    let report = RepoReport {
        repo_name: tree::repo_display_name(&config.repo_path),
        scanned_commits: history.scanned_commits,
        file_histories: history.file_histories,
        directory_histories: history.directory_histories,
        tree,
        agent_profile: config.agent_profile,
        detected_technologies,
        added_skills: skills_result.added_skills,
        skills_manifest_href: skills_result.skills_manifest_href,
        include_patterns: config
            .generation_options
            .matcher
            .include_patterns()
            .to_vec(),
        exclude_patterns: config
            .generation_options
            .matcher
            .exclude_patterns()
            .to_vec(),
        output_formats: config.output_formats,
    };

    write_outputs(&config, &report)?;

    let mut artifacts = Vec::new();
    if report.output_formats.includes_markdown() {
        artifacts.push("markdown");
    }
    if report.output_formats.includes_html() {
        artifacts.push("HTML");
    }
    if report.output_formats.includes_json() {
        artifacts.push("JSON");
    }

    println!(
        "Wrote {} file summaries, {} folder summaries, and {} artifacts under {}",
        report.file_histories.len(),
        report.changed_directories(),
        artifacts.join("/"),
        config.output_dir.display()
    );

    Ok(())
}

fn write_outputs(config: &Config, report: &RepoReport) -> AppResult<()> {
    fs::create_dir_all(&config.output_dir)
        .map_err(|error| AppError::io("failed to create output directory", error))?;

    if report.output_formats.includes_markdown() {
        fs::create_dir_all(config.output_dir.join("files"))
            .map_err(|error| AppError::io("failed to create files directory", error))?;
        fs::create_dir_all(config.output_dir.join("dirs"))
            .map_err(|error| AppError::io("failed to create dirs directory", error))?;

        fs::write(
            config.output_dir.join("SUMMARY.md"),
            render_markdown::render_summary(report),
        )
        .map_err(|error| AppError::io("failed to write summary", error))?;

        for file in report.sorted_file_histories() {
            let destination = model::markdown_path(&config.output_dir, &file.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    AppError::io(
                        format!("failed to create directory for {}", file.path),
                        error,
                    )
                })?;
            }

            fs::write(
                &destination,
                render_markdown::render_file_summary(report, file),
            )
            .map_err(|error| {
                AppError::io(format!("failed to write {}", destination.display()), error)
            })?;
        }

        for directory in report
            .sorted_directory_histories()
            .into_iter()
            .filter(|directory| !directory.path.is_empty())
        {
            let destination = model::directory_markdown_path(&config.output_dir, &directory.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    AppError::io(
                        format!("failed to create directory for {}", directory.path),
                        error,
                    )
                })?;
            }

            fs::write(
                &destination,
                render_markdown::render_directory_summary(report, directory),
            )
            .map_err(|error| {
                AppError::io(
                    format!(
                        "failed to write directory summary {}",
                        destination.display()
                    ),
                    error,
                )
            })?;
        }
    }

    if report.output_formats.includes_html() {
        fs::write(
            config.output_dir.join("index.html"),
            render_html::render_html_viewer(report)?,
        )
        .map_err(|error| AppError::io("failed to write index.html", error))?;
    }

    if report.output_formats.includes_json() {
        let json = serde_json::to_string_pretty(&report.to_bundle())
            .map_err(|error| AppError::json("failed to serialize report.json", error))?;
        fs::write(config.output_dir.join("report.json"), json)
            .map_err(|error| AppError::io("failed to write report.json", error))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
