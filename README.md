# history-to-md

Rust CLI that turns a repository's git history into markdown summaries and a browsable HTML viewer.

## What it generates

For a target repository, the binary now:

- reads `git log --numstat`
- aggregates commit count, churn, authors, and recent commits per file
- aggregates the same history per folder
- walks the live repository tree so unchanged files and folders still appear in the UI
- writes `SUMMARY.md`
- writes one markdown file per changed file under `files/`
- writes one markdown file per changed folder under `dirs/`
- writes `index.html`, a self-contained viewer over the project structure
- tags each markdown file with the selected agent profile and a small markdown format contract

The HTML viewer lets you click a folder or file and see:

- the repo structure
- relevant generated markdown reports for that node
- the full commit history that touched that folder or file
- churn and primary author information

## Usage

```bash
cargo run -- /path/to/repository
```

Optional output directory:

```bash
cargo run -- /path/to/repository /path/to/output
```

Optional agent profile:

```bash
cargo run -- --agent codex /path/to/repository
```

Supported agent profiles: `generic`, `codex`, `claude`, `cursor`, `aider`.

With the default output location, the tool writes into `/path/to/repository/history-md`.

The selected profile is written into the YAML frontmatter of every generated markdown file and shown in the HTML viewer, so you can tell which markdown shape the output is targeting.

## Examples

Run against the current repository and write into a separate folder:

```bash
cargo run -- --agent codex . ./tmp/history-md
```

Generate into the repository default output folder with the generic profile:

```bash
cargo run -- .
```

Generated markdown starts with YAML frontmatter plus an agent-format contract. A file summary looks like:

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

# src/main.rs

## Agent Format

- Target agent: Codex
- Markdown style: Direct engineering-oriented sections, flat bullets, and code identifiers kept in backticks.
- Usage hint: Use when the reader is a coding agent that prefers terse, operational context.
```

## Output layout

```text
history-md/
  index.html
  SUMMARY.md
  files/
  dirs/
```

Open `history-md/index.html` in a browser to inspect the project tree and jump into the generated markdown.
