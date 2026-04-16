use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn fixture_repository_matches_snapshots() {
    let temp_root = unique_temp_path("history-to-md-fixture");
    let repo_path = temp_root.join("repo");
    let output_dir = temp_root.join("out");
    fs::create_dir_all(&repo_path).expect("temp repo path should be created");
    copy_dir_recursive(&fixture_root("basic_repo"), &repo_path);

    init_git_repository(&repo_path, "Fixture User", "fixture@example.com");
    git_commit(
        &repo_path,
        "Initial fixture import",
        "Fixture User",
        "fixture@example.com",
        "2026-01-02T10:00:00+00:00",
    );

    fs::write(
        repo_path.join("README.md"),
        "# fixture-app\n\nSeed fixture repository for integration tests.\n\nupdated\n",
    )
    .expect("fixture README should update");
    fs::write(
        repo_path.join("src/main.rs"),
        "fn helper() -> usize {\n    7\n}\n\nfn main() {\n    println!(\"fixture {}\", helper());\n}\n",
    )
    .expect("fixture main should update");
    fs::create_dir_all(repo_path.join("src/utils")).expect("fixture utils dir should exist");
    fs::write(
        repo_path.join("src/utils/mod.rs"),
        "pub fn meaning() -> usize { 42 }\n",
    )
    .expect("fixture utils module should exist");
    git_commit(
        &repo_path,
        "Add helper module",
        "Jane Doe",
        "jane@example.com",
        "2026-02-03T11:30:00+00:00",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_history-to-md"))
        .args([
            "--agent",
            "codex",
            repo_path.to_str().expect("repo path should be utf-8"),
            output_dir.to_str().expect("output path should be utf-8"),
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "binary failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_snapshot(
        &output_dir.join("SUMMARY.md"),
        &snapshot_root("basic/SUMMARY.md"),
    );
    assert_snapshot(
        &output_dir.join("files/src/main.rs.md"),
        &snapshot_root("basic/files_src_main.rs.md"),
    );
    assert_snapshot(
        &output_dir.join("dirs/src/INDEX.md"),
        &snapshot_root("basic/dirs_src_INDEX.md"),
    );
    assert_snapshot(
        &output_dir.join("report.json"),
        &snapshot_root("basic/report.json"),
    );

    let html = fs::read_to_string(output_dir.join("index.html")).expect("html should be readable");
    assert!(html.contains("Markdown profile: Codex"));
    assert!(html.contains("\"available_formats\":[\"md\",\"html\",\"json\"]"));
    assert!(html.contains("Repository summary"));

    fs::remove_dir_all(&temp_root).expect("temp fixture path should be cleaned up");
}

fn fixture_root(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

fn snapshot_root(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/snapshots")
        .join(path)
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nonce}"))
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).expect("fixture dir should be readable") {
        let entry = entry.expect("fixture entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            fs::create_dir_all(&destination_path).expect("fixture directory should be copied");
            copy_dir_recursive(&source_path, &destination_path);
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).expect("fixture parent should exist");
            }
            fs::copy(&source_path, &destination_path).expect("fixture file should be copied");
        }
    }
}

fn init_git_repository(repo_path: &Path, user_name: &str, user_email: &str) {
    run_git(repo_path, &["init"], &[]);
    run_git(repo_path, &["config", "user.name", user_name], &[]);
    run_git(repo_path, &["config", "user.email", user_email], &[]);
}

fn git_commit(repo_path: &Path, message: &str, author_name: &str, author_email: &str, date: &str) {
    run_git(repo_path, &["add", "."], &[]);
    run_git(
        repo_path,
        &["commit", "-m", message],
        &[
            ("GIT_AUTHOR_NAME", author_name),
            ("GIT_AUTHOR_EMAIL", author_email),
            ("GIT_COMMITTER_NAME", author_name),
            ("GIT_COMMITTER_EMAIL", author_email),
            ("GIT_AUTHOR_DATE", date),
            ("GIT_COMMITTER_DATE", date),
        ],
    );
}

fn run_git(repo_path: &Path, args: &[&str], envs: &[(&str, &str)]) {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_path).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }

    let output = command.output().expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_snapshot(actual_path: &Path, expected_path: &Path) {
    let actual = fs::read_to_string(actual_path).expect("actual snapshot should be readable");
    let expected = fs::read_to_string(expected_path).expect("expected snapshot should be readable");
    assert_eq!(normalize_newlines(&actual), normalize_newlines(&expected));
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").trim_end().to_string()
}
