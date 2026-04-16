use crate::error::{AppError, AppResult};
use crate::model::{PathHistory, RepoReport, TreeNode};
use crate::render_markdown::{directory_summary_link, summary_link};
use crate::tree::{display_path, specific_directory_chain};
use serde::Serialize;
use std::fmt::Write as _;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReportLink {
    pub label: String,
    pub href: String,
}

pub fn render_html_viewer(report: &RepoReport) -> AppResult<String> {
    let serialized_data = serialize_for_html(&report.to_bundle())?;

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
        report.changed_files(),
        report.changed_directories(),
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

pub fn relevant_report_links(node: &TreeNode, report: &RepoReport) -> Vec<ReportLink> {
    let mut links = Vec::new();

    if node.path.is_empty()
        && let Some(manifest_href) = report.skills_manifest_href.as_ref()
    {
        links.push(ReportLink {
            label: "Skills manifest".to_string(),
            href: manifest_href.clone(),
        });
    }

    if report.output_formats.includes_markdown() {
        if !node.is_dir && report.file_history(&node.path).is_some() {
            links.push(ReportLink {
                label: "File history".to_string(),
                href: summary_link(&node.path),
            });
        }

        for directory in specific_directory_chain(&node.path, node.is_dir) {
            if report.directory_history(&directory).is_some() {
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
    }

    links
}

pub fn serialize_for_html<T: Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value)
        .map(|json| json.replace("</", "<\\/"))
        .map_err(|error| AppError::json("failed to serialize viewer data", error))
}

fn render_tree_html(node: &TreeNode, report: &RepoReport, depth: usize) -> String {
    let history = node_history(node, report);

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

fn node_history<'a>(node: &TreeNode, report: &'a RepoReport) -> Option<&'a PathHistory> {
    if node.is_dir {
        report.directory_history(&node.path)
    } else {
        report.file_history(&node.path)
    }
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
const fileHistories = new Map(data.file_histories.map((history) => [history.path, history]));
const directoryHistories = new Map(data.directory_histories.map((history) => [history.path, history]));
const detailPanel = document.getElementById("node-details");
const buttons = Array.from(document.querySelectorAll("[data-node-path]"));
const markdownEnabled = data.available_formats.includes("md");

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

function markdownPath(filePath) {
  const parts = filePath.split("/").filter(Boolean);
  const last = parts.pop() || "";
  const extIndex = last.lastIndexOf(".");
  const fileName = extIndex >= 0 ? `${last}.md` : `${last}.md`;
  return `files/${[...parts, fileName].join("/")}`;
}

function directoryMarkdownPath(dirPath) {
  return `dirs/${dirPath}/INDEX.md`.replace(/^dirs\/$/, "dirs/INDEX.md");
}

function topAuthors(history, limit = 5) {
  const entries = Object.entries(history.authors || {});
  entries.sort((left, right) => {
    const countDiff = right[1] - left[1];
    return countDiff !== 0 ? countDiff : left[0].localeCompare(right[0]);
  });
  const rendered = entries
    .slice(0, limit)
    .map(([author, count]) => `${author} (${count})`)
    .join(", ");
  return rendered || "n/a";
}

function specificDirectoryChain(path, isDir) {
  const parts = path.split("/").filter(Boolean);
  const directoryCount = isDir ? parts.length : Math.max(parts.length - 1, 0);
  const directories = [];
  for (let index = 0; index < directoryCount; index += 1) {
    directories.push(parts.slice(0, index + 1).join("/"));
  }
  return directories.reverse();
}

function relevantLinks(node, history) {
  const links = [];
  if (node.path === "" && data.skills_manifest_href) {
    links.push({ label: "Skills manifest", href: data.skills_manifest_href });
  }
  if (!markdownEnabled) {
    return links;
  }
  if (!node.is_dir && history) {
    links.push({ label: "File history", href: markdownPath(node.path) });
  }
  specificDirectoryChain(node.path, node.is_dir).forEach((directory) => {
    if (directoryHistories.has(directory)) {
      links.push({
        label: `Folder history: ${directory}`,
        href: directoryMarkdownPath(directory),
      });
    }
  });
  links.push({ label: "Repository summary", href: "SUMMARY.md" });
  return links;
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

function findNode(path, node = data.tree) {
  if (node.path === path) {
    return node;
  }
  for (const child of node.children || []) {
    const match = findNode(path, child);
    if (match) {
      return match;
    }
  }
  return null;
}

function renderNode(path) {
  const node = findNode(path);
  if (!node) {
    detailPanel.innerHTML = `<p class="empty-state">No data found for this node.</p>`;
    return;
  }

  buttons.forEach((button) => {
    button.classList.toggle("node-button-selected", button.dataset.nodePath === path);
  });

  const history = node.is_dir ? directoryHistories.get(path) : fileHistories.get(path);
  const commits = history && history.commits.length
    ? history.commits
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

  const reportLinks = relevantLinks(node, history);
  const renderedLinks = reportLinks.length
    ? reportLinks
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

  const commitCount = history ? history.commit_count : 0;
  const totalAdded = history ? history.total_added : 0;
  const totalDeleted = history ? history.total_deleted : 0;
  const primaryAuthors = history ? topAuthors(history, 5) : "n/a";

  detailPanel.innerHTML = `
    <div class="detail-header">
      <div>
        <p class="eyebrow">${node.is_dir ? "Folder" : "File"}</p>
        <h2>${escapeHtml(node.name)}</h2>
        <p class="detail-path">${escapeHtml(node.path || "/")}</p>
      </div>
      <span class="badge">${pluralize(commitCount, "commit")}</span>
    </div>
    <div class="stat-grid">
      <div class="stat-card">
        <span class="stat-label">Added</span>
        <strong class="stat-value">${totalAdded}</strong>
      </div>
      <div class="stat-card">
        <span class="stat-label">Deleted</span>
        <strong class="stat-value">${totalDeleted}</strong>
      </div>
      <div class="stat-card">
        <span class="stat-label">Primary Authors</span>
        <strong class="stat-value">${escapeHtml(primaryAuthors)}</strong>
      </div>
    </div>
    <section class="list-card">
      <h3 class="section-title">Relevant Markdown</h3>
      <ul class="link-list">${renderedLinks}</ul>
    </section>
    ${rootSections}
    <section class="list-card" style="margin-top: 16px;">
      <h3 class="section-title">Commit History (${commitCount})</h3>
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
