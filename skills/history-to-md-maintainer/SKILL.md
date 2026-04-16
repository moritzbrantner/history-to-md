---
name: history-to-md-maintainer
description: Use this skill when working in the history-to-md repository and you are writing commit messages, change summaries, or other notes that should stay machine-parseable in git history.
---

# History To MD Maintainer

Use this skill for work in this repository when you write:

- commit subjects
- implementation summaries
- review summaries
- handoff notes meant to be preserved in git history or later scraped from markdown

## Goal

This repository turns git history into markdown. Important facts must therefore use stable, parseable wording instead of decorative emphasis or loose prose.

Do not write freeform markers like:

- `!!! do not use yarn, but pnpm !!!`

Write a machine-friendly marker instead:

- `H2M-TOOLING: use pnpm, never yarn`

## Required marker format

Use this exact pattern:

`H2M-<TAG>: <single fact>`

Rules:

- Keep the `H2M-` prefix and tag uppercase.
- Keep one fact per marker line.
- Prefer short, literal statements over clever phrasing.
- Use ASCII only unless the surrounding file already requires non-ASCII text.
- If a fact matters for git-history parsing, put it in the commit subject, not only in the body.

## Allowed tags

- `H2M-TOOLING:` package managers, runtimes, CLIs, build tools
- `H2M-DECISION:` durable project decisions
- `H2M-WARNING:` things contributors must not do
- `H2M-BREAKING:` behavior changes that can break users or automation
- `H2M-MIGRATION:` required follow-up changes or upgrade steps

## Commit message rules

When a commit contains an important fact that downstream parsing should detect:

- Start the commit subject with the marker.
- Put the most important fact first.
- If the commit also needs human context, add that after the marker with ` - `.

Examples:

- `H2M-TOOLING: use pnpm, never yarn`
- `H2M-WARNING: do not parse commit bodies - history extraction reads subjects`
- `H2M-BREAKING: rename --skills-db output field to skills_manifest_href`

Avoid:

- `chore: update package manager`
- `!!! switch to pnpm !!!`
- `important: don't use yarn`

## Summary rules

When writing summaries in comments, reviews, or handoff notes:

- Put each parseable fact on its own line or bullet.
- Preserve the exact marker prefix.
- Keep the rest of the summary normal and concise.

Example:

- `H2M-TOOLING: use pnpm, never yarn`
- `H2M-DECISION: keep parser rules based on commit subjects`

## When not to force a marker

Do not convert every sentence into a marker. Use markers only for facts that should remain easy to extract from git history or generated markdown later.
