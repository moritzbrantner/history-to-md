use crate::error::{AppError, AppResult};
use crate::model::{
    AgentProfile, DEFAULT_OUTPUT_DIR, GenerationOptions, OutputFormats, PathMatcher,
};
use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillsDatabaseConfig {
    pub database_path: PathBuf,
    pub install_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub repo_path: PathBuf,
    pub output_dir: PathBuf,
    pub agent_profile: AgentProfile,
    pub skills_database: Option<SkillsDatabaseConfig>,
    pub generation_options: GenerationOptions,
    pub output_formats: OutputFormats,
}

impl Config {
    pub fn from_env() -> AppResult<Self> {
        let args = env::args().collect::<Vec<_>>();
        Self::from_args(&args)
    }

    pub fn from_args(args: &[String]) -> AppResult<Self> {
        let program_name = args.first().map_or("history-to-md", String::as_str);
        let mut positionals = Vec::new();
        let mut agent_profile = AgentProfile::Generic;
        let mut skills_database_path: Option<PathBuf> = None;
        let mut skills_install_dir: Option<PathBuf> = None;
        let mut since = None;
        let mut until = None;
        let mut max_commits = None;
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        let mut output_formats = OutputFormats::default();
        let mut index = 1;

        while index < args.len() {
            match args[index].as_str() {
                "--agent" => {
                    let value = expect_value(args, index, "--agent", program_name)?;
                    agent_profile = AgentProfile::parse(value).map_err(AppError::message)?;
                    index += 2;
                }
                "--skills-db" => {
                    let value = expect_value(args, index, "--skills-db", program_name)?;
                    skills_database_path = Some(PathBuf::from(value));
                    index += 2;
                }
                "--skills-dir" => {
                    let value = expect_value(args, index, "--skills-dir", program_name)?;
                    skills_install_dir = Some(PathBuf::from(value));
                    index += 2;
                }
                "--since" => {
                    let value = expect_value(args, index, "--since", program_name)?;
                    validate_date_literal(value)?;
                    since = Some(value.to_string());
                    index += 2;
                }
                "--until" => {
                    let value = expect_value(args, index, "--until", program_name)?;
                    validate_date_literal(value)?;
                    until = Some(value.to_string());
                    index += 2;
                }
                "--max-commits" => {
                    let value = expect_value(args, index, "--max-commits", program_name)?;
                    let parsed = value.parse::<usize>().map_err(|error| {
                        AppError::parse_int("invalid value for --max-commits", error)
                    })?;
                    if parsed == 0 {
                        return Err(AppError::message(
                            "invalid value for --max-commits: expected a positive integer",
                        ));
                    }
                    max_commits = Some(parsed);
                    index += 2;
                }
                "--include" => {
                    let value = expect_value(args, index, "--include", program_name)?;
                    includes.push(value.to_string());
                    index += 2;
                }
                "--exclude" => {
                    let value = expect_value(args, index, "--exclude", program_name)?;
                    excludes.push(value.to_string());
                    index += 2;
                }
                "--formats" => {
                    let value = expect_value(args, index, "--formats", program_name)?;
                    output_formats = OutputFormats::parse(value).map_err(AppError::message)?;
                    index += 2;
                }
                argument if argument.starts_with("--") => {
                    return Err(AppError::message(format!(
                        "unknown option: {argument}\n{}",
                        usage(program_name)
                    )));
                }
                argument => {
                    positionals.push(argument.to_string());
                    index += 1;
                }
            }
        }

        if positionals.is_empty() || positionals.len() > 2 {
            return Err(AppError::message(usage(program_name)));
        }

        let repo_path = PathBuf::from(&positionals[0]);
        if !repo_path.exists() {
            return Err(AppError::message(format!(
                "repository path does not exist: {}",
                repo_path.display()
            )));
        }

        let output_dir = positionals
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_path.join(DEFAULT_OUTPUT_DIR));

        let skills_database = match skills_database_path {
            Some(database_path) => {
                if !database_path.exists() {
                    return Err(AppError::message(format!(
                        "skills database path does not exist: {}",
                        database_path.display()
                    )));
                }

                let install_dir = skills_install_dir.unwrap_or_else(|| output_dir.join("skills"));
                Some(SkillsDatabaseConfig {
                    database_path,
                    install_dir,
                })
            }
            None => {
                if let Some(install_dir) = skills_install_dir {
                    return Err(AppError::message(format!(
                        "--skills-dir requires --skills-db (got {})",
                        install_dir.display()
                    )));
                }
                None
            }
        };

        let matcher = PathMatcher::new(includes, excludes).map_err(AppError::message)?;

        Ok(Self {
            repo_path,
            output_dir,
            agent_profile,
            skills_database,
            generation_options: GenerationOptions {
                since,
                until,
                max_commits,
                matcher,
            },
            output_formats,
        })
    }
}

fn expect_value<'a>(
    args: &'a [String],
    index: usize,
    flag: &str,
    program_name: &str,
) -> AppResult<&'a str> {
    args.get(index + 1).map(String::as_str).ok_or_else(|| {
        AppError::message(format!("missing value for {flag}\n{}", usage(program_name)))
    })
}

fn usage(program_name: &str) -> String {
    format!(
        "usage: {program_name} [--agent <{}>] [--skills-db <path>] [--skills-dir <path>] [--since <YYYY-MM-DD>] [--until <YYYY-MM-DD>] [--max-commits <N>] [--include <glob>]... [--exclude <glob>]... [--formats <md,html,json>] <repo-path> [output-dir]",
        AgentProfile::supported_names().join("|")
    )
}

fn validate_date_literal(value: &str) -> AppResult<()> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(AppError::message(format!(
            "invalid date for value `{value}`: expected YYYY-MM-DD"
        )));
    }

    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return Err(AppError::message(format!(
            "invalid date for value `{value}`: expected YYYY-MM-DD"
        )));
    }

    let year = value[0..4]
        .parse::<u32>()
        .map_err(|error| AppError::parse_int("invalid year in date literal", error))?;
    let month = value[5..7]
        .parse::<u32>()
        .map_err(|error| AppError::parse_int("invalid month in date literal", error))?;
    let day = value[8..10]
        .parse::<u32>()
        .map_err(|error| AppError::parse_int("invalid day in date literal", error))?;

    if year == 0 || month == 0 || month > 12 {
        return Err(AppError::message(format!(
            "invalid date for value `{value}`: expected YYYY-MM-DD"
        )));
    }

    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => unreachable!(),
    };

    if day == 0 || day > max_day {
        return Err(AppError::message(format!(
            "invalid date for value `{value}`: expected YYYY-MM-DD"
        )));
    }

    Ok(())
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
