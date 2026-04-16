# history-to-md

Rust CLI that turns a repository's git history into markdown summaries, a browsable HTML viewer, and a machine-readable JSON report.

## Install

Build locally:

```bash
cargo build --release
```

Run without installing:

```bash
cargo run -- /path/to/repository
```

Install from the current checkout:

```bash
cargo install --path .
```

## Compatibility

- Supported interface: CLI only
- Runtime dependencies: `git` must be available on `PATH`
- Platform target: developed and tested on Linux; release builds are produced for Linux, macOS, and Windows
- Input model: local git repositories only; no network services are required

## Project Policy

- `H2M-DECISION: prioritize the CLI as the only supported public interface`
- `H2M-DECISION: keep library internals unstable and free to refactor`
- `H2M-TOOLING: require cargo fmt --check, cargo test, and cargo clippy --all-targets --all-features -- -D warnings`
- `H2M-BREAKING: document any non-additive CLI or output contract changes with an explicit H2M marker`

## What It Generates

For a target repository, the binary can:

- read `git log --numstat`
- aggregate commit count, churn, authors, and recent commits per file
- aggregate the same history per folder
- walk the live repository tree so unchanged files and folders still appear in the UI
- write `SUMMARY.md`
- write one markdown file per changed file under `files/`
- write one markdown file per changed folder under `dirs/`
- write `index.html`, a self-contained viewer over the project structure
- write `report.json`, a machine-readable report bundle
- tag every markdown file with the selected agent profile and a small markdown format contract
- optionally detect technologies and install matching skills from a skills database

The HTML viewer lets you inspect:

- the repo structure
- generated reports for a node
- the full commit history that touched that folder or file
- churn and primary author information
- detected technologies and copied skills at the repository root

## Usage

Basic invocation:

```bash
history-to-md /path/to/repository
```

Custom output directory:

```bash
history-to-md /path/to/repository /path/to/output
```

Agent profile:

```bash
history-to-md --agent codex /path/to/repository
```

History window and path filters:

```bash
history-to-md \
  --since 2026-01-01 \
  --until 2026-03-31 \
  --max-commits 200 \
  --include 'src/**' \
  --exclude 'src/generated/**' \
  /path/to/repository
```

Select generated artifacts:

```bash
history-to-md --formats md,json /path/to/repository
```

Skills database:

```bash
history-to-md --agent codex --skills-db ./skills-database.example.json .
```

Custom skills install destination:

```bash
history-to-md \
  --skills-db ./skills-database.example.json \
  --skills-dir ~/.codex/skills \
  .
```

With the default output location, the tool writes into `/path/to/repository/history-md`.

Supported agent profiles: `generic`, `codex`, `claude`, `cursor`, `aider`.

Supported output formats: `md`, `html`, `json`.

## CLI Reference

```text
history-to-md
  [--agent <generic|codex|claude|cursor|aider>]
  [--skills-db <path>]
  [--skills-dir <path>]
  [--since <YYYY-MM-DD>]
  [--until <YYYY-MM-DD>]
  [--max-commits <N>]
  [--include <glob>]...
  [--exclude <glob>]...
  [--formats <md,html,json>]
  <repo-path>
  [output-dir]
```

Flag notes:

- `--since`, `--until`: limit the git log window
- `--max-commits`: cap the number of scanned commits
- `--include`: repeatable glob filter for included repo-relative paths
- `--exclude`: repeatable glob filter for excluded repo-relative paths
- `--formats`: comma-separated artifact selection; default is `md,html,json`
- `--skills-dir`: only valid when `--skills-db` is also provided

## Output Layout

```text
history-md/
  index.html
  report.json
  SUMMARY.md
  files/
  dirs/
  skills/          # only when --skills-db is used with the default install dir
```

Open `history-md/index.html` in a browser to inspect the project tree and jump into generated reports.

## Output Contract

`report.json` schema version `1` includes these top-level fields:

- `generated_by`
- `format_version`
- `repo_name`
- `agent_profile`
- `agent_display_name`
- `scanned_commits`
- `detected_technologies`
- `added_skills`
- `skills_manifest_href`
- `tree`
- `file_histories`
- `directory_histories`
- `available_formats`

Markdown output includes YAML frontmatter plus an agent-format contract. A file summary starts like:

```md
---
generated_by: history-to-md
format_version: 1
agent_profile: codex
agent_display_name: 'Codex'
document_kind: file-history
repo_name: 'history-to-md'
title: 'src/main.rs'
path: 'src/main.rs'
---
```

## Versioning Policy

- New CLI flags should be additive by default.
- New `report.json` fields should be additive by default.
- If a change can break automation or downstream parsing, record it with an `H2M-BREAKING:` marker.
- If users need follow-up steps, record them with an `H2M-MIGRATION:` marker.

## Skills Database Format

The skills database is a JSON file. Paths in `source` are resolved relative to the database file.

```json
{
  "skills": [
    {
      "id": "rust-review",
      "title": "Rust Review",
      "description": "Rust-specific review guidance.",
      "technologies": ["rust"],
      "source": "skills/rust-review"
    },
    {
      "id": "frontend-review",
      "title": "Frontend Review",
      "description": "Checks for React and TypeScript projects.",
      "technologies": ["react", "typescript"],
      "match_mode": "all",
      "source": "skills/frontend-review",
      "install_as": "frontend-review"
    }
  ]
}
```

Field notes:

- `technologies` is matched against the built-in detector ids
- `match_mode` is optional and defaults to `any`
- `source` can point to either a file or a directory
- `install_as` is optional and defaults to the skill `id`

## Built-In Technology Detection

Current built-in detections:

- `docker`
- `go`
- `java`
- `javascript`
- `kotlin`
- `kubernetes`
- `nodejs`
- `python`
- `react`
- `rust`
- `terraform`
- `typescript`
