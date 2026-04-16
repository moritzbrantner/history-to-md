use crate::error::{AppError, AppResult};
use crate::model::{DetectedTechnology, TreeNode};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn detect_technologies(
    repo_path: &Path,
    tree: &TreeNode,
) -> AppResult<Vec<DetectedTechnology>> {
    let mut files = Vec::new();
    collect_file_paths(tree, &mut files);
    let file_set: HashSet<&str> = files.iter().map(String::as_str).collect();
    let mut detected = Vec::new();

    push_detected_technology(
        &mut detected,
        "docker",
        "Docker",
        vec![
            find_exact_path(&file_set, &files, "Dockerfile"),
            find_suffix_path(&files, ".dockerfile"),
            find_exact_path(&file_set, &files, "docker-compose.yml"),
            find_exact_path(&file_set, &files, "docker-compose.yaml"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "go",
        "Go",
        vec![
            find_exact_path(&file_set, &files, "go.mod"),
            find_path_with_extension(&files, "go"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "java",
        "Java",
        vec![
            find_exact_path(&file_set, &files, "pom.xml"),
            find_exact_path(&file_set, &files, "build.gradle"),
            find_path_with_extension(&files, "java"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "javascript",
        "JavaScript",
        vec![
            find_exact_path(&file_set, &files, "package.json"),
            find_path_with_extension(&files, "js"),
            find_path_with_extension(&files, "jsx"),
            find_path_with_extension(&files, "mjs"),
            find_path_with_extension(&files, "cjs"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "kotlin",
        "Kotlin",
        vec![
            find_exact_path(&file_set, &files, "build.gradle.kts"),
            find_path_with_extension(&files, "kt"),
            find_path_with_extension(&files, "kts"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "kubernetes",
        "Kubernetes",
        vec![
            find_exact_path(&file_set, &files, "Chart.yaml"),
            find_exact_path(&file_set, &files, "kustomization.yaml"),
            find_exact_path(&file_set, &files, "kustomization.yml"),
            find_prefix_path(&files, "k8s/"),
            find_prefix_path(&files, "helm/"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "nodejs",
        "Node.js",
        vec![find_exact_path(&file_set, &files, "package.json")],
    );
    push_detected_technology(
        &mut detected,
        "python",
        "Python",
        vec![
            find_exact_path(&file_set, &files, "pyproject.toml"),
            find_exact_path(&file_set, &files, "requirements.txt"),
            find_exact_path(&file_set, &files, "setup.py"),
            find_path_with_extension(&files, "py"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "rust",
        "Rust",
        vec![
            find_exact_path(&file_set, &files, "Cargo.toml"),
            find_path_with_extension(&files, "rs"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "terraform",
        "Terraform",
        vec![
            find_path_with_extension(&files, "tf"),
            find_path_with_extension(&files, "tfvars"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "typescript",
        "TypeScript",
        vec![
            find_exact_path(&file_set, &files, "tsconfig.json"),
            find_path_with_extension(&files, "ts"),
            find_path_with_extension(&files, "tsx"),
        ],
    );
    push_detected_technology(
        &mut detected,
        "react",
        "React",
        vec![
            file_contains_any(
                repo_path,
                "package.json",
                &["\"react\"", "\"next\"", "\"@types/react\""],
            )?,
            find_path_with_extension(&files, "jsx"),
            find_path_with_extension(&files, "tsx"),
        ],
    );

    detected.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(detected)
}

fn collect_file_paths(node: &TreeNode, files: &mut Vec<String>) {
    if node.is_dir {
        for child in &node.children {
            collect_file_paths(child, files);
        }
        return;
    }

    if !node.path.is_empty() {
        files.push(node.path.clone());
    }
}

fn push_detected_technology(
    detected: &mut Vec<DetectedTechnology>,
    id: &str,
    name: &str,
    evidence: Vec<Option<String>>,
) {
    let evidence = evidence.into_iter().flatten().collect::<Vec<_>>();
    if evidence.is_empty() {
        return;
    }

    detected.push(DetectedTechnology {
        id: id.to_string(),
        name: name.to_string(),
        evidence,
    });
}

fn find_exact_path(file_set: &HashSet<&str>, files: &[String], exact_path: &str) -> Option<String> {
    if file_set.contains(exact_path) {
        return Some(format!("Found `{exact_path}`"));
    }

    files
        .iter()
        .find(|path| path.ends_with(&format!("/{exact_path}")))
        .map(|path| format!("Found `{path}`"))
}

fn find_prefix_path(files: &[String], prefix: &str) -> Option<String> {
    files
        .iter()
        .find(|path| path.starts_with(prefix))
        .map(|path| format!("Found `{path}`"))
}

fn find_suffix_path(files: &[String], suffix: &str) -> Option<String> {
    files
        .iter()
        .find(|path| path.ends_with(suffix))
        .map(|path| format!("Found `{path}`"))
}

fn find_path_with_extension(files: &[String], extension: &str) -> Option<String> {
    let expected = format!(".{extension}");
    files
        .iter()
        .find(|path| path.ends_with(&expected))
        .map(|path| format!("Found `{path}`"))
}

fn file_contains_any(
    repo_path: &Path,
    relative_path: &str,
    needles: &[&str],
) -> AppResult<Option<String>> {
    let file_path = repo_path.join(relative_path);
    if !file_path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&file_path).map_err(|error| {
        AppError::io(
            format!(
                "failed to read technology marker file {}",
                file_path.display()
            ),
            error,
        )
    })?;

    Ok(needles
        .iter()
        .find(|needle| contents.contains(**needle))
        .map(|needle| format!("Found `{relative_path}` containing `{needle}`")))
}
