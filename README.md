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

Optional skills database:

```bash
cargo run -- --agent codex --skills-db ./skills-database.example.json .
```

Optional custom install destination for matched skills:

```bash
cargo run -- --skills-db ./skills-database.example.json --skills-dir ~/.codex/skills .
```

When a skills database is provided, the tool also:

- detects common technologies in the target repository from its live tree
- matches those detections against the database entries
- copies matching skill folders or files into the configured skills directory
- writes a `manifest.json` for the copied skills
- includes detected technologies and copied skills in `SUMMARY.md` and `index.html`

Built-in technology detection currently recognizes: `docker`, `go`, `java`, `javascript`, `kotlin`, `kubernetes`, `nodejs`, `python`, `react`, `rust`, `terraform`, `typescript`.

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
  skills/          # only when --skills-db is used with the default install dir
```

Open `history-md/index.html` in a browser to inspect the project tree and jump into the generated markdown.

## Skills database format

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

- `technologies` is matched against the built-in detector ids.
- `match_mode` is optional and defaults to `any`. Use `all` when every listed technology must be present.
- `source` can point to either a file or a directory. Directory entries usually contain a `SKILL.md`.
- `install_as` is optional and defaults to the skill `id`.
