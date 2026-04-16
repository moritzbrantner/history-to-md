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

With the default output location, the tool writes into `/path/to/repository/history-md`.

## Output layout

```text
history-md/
  index.html
  SUMMARY.md
  files/
  dirs/
```

Open `history-md/index.html` in a browser to inspect the project tree and jump into the generated markdown.
