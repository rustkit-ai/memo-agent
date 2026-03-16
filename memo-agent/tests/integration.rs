use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn memo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_memo"))
}

fn temp_home(test_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "memo_itest_{}_{}",
        test_name,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp home");
    // Also create a fake project dir inside
    std::fs::create_dir_all(dir.join("project")).expect("create project dir");
    dir
}

fn run_memo(home: &PathBuf, args: &[&str]) -> std::process::Output {
    run_memo_with_bin_on_path(home, args, false)
}

fn run_memo_with_bin_on_path(home: &PathBuf, args: &[&str], bin_on_path: bool) -> std::process::Output {
    let bin = memo_bin();
    let mut cmd = Command::new(&bin);
    cmd.args(args)
        .env("HOME", home)
        // Use project subdir so project_id is consistent per test
        .current_dir(home.join("project"))
        // Prevent git remote lookups from going to unrelated repos
        .env("GIT_DIR", "/dev/null");
    if bin_on_path {
        let bin_dir = bin.parent().expect("binary has parent dir");
        let path_env = std::env::var("PATH").unwrap_or_default();
        let sep = if cfg!(windows) { ";" } else { ":" };
        let new_path = format!("{}{sep}{}", bin_dir.display(), path_env);
        cmd.env("PATH", new_path);
    }
    cmd.output().expect("failed to run memo")
}

#[test]
fn test_log_and_list() {
    let home = temp_home("log_list");

    let out = run_memo(&home, &["log", "hello world"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "memo log failed: {:?}", out);
    assert!(stdout.contains("logged: hello world"), "unexpected output: {}", stdout);

    let list_out = run_memo(&home, &["list"]);
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(list_out.status.success(), "memo list failed: {:?}", list_out);
    assert!(list_stdout.contains("hello world"), "entry not in list: {}", list_stdout);
}

#[test]
fn test_search() {
    let home = temp_home("search");

    run_memo(&home, &["log", "findme needle in haystack"]);
    run_memo(&home, &["log", "unrelated entry"]);

    let out = run_memo(&home, &["search", "needle"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "memo search failed: {:?}", out);
    assert!(stdout.contains("findme needle"), "search didn't find entry: {}", stdout);
    assert!(!stdout.contains("unrelated"), "search returned unrelated entry: {}", stdout);
}

#[test]
fn test_delete() {
    let home = temp_home("delete");

    run_memo(&home, &["log", "entry to delete"]);

    // Get the ID from list output
    let list_out = run_memo(&home, &["list"]);
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);

    // Parse id from output: "#<id> ..."
    let id_str = list_stdout
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#').and_then(|rest| rest.split_whitespace().next())
        })
        .next()
        .expect("no entry id found in list output");

    let delete_out = run_memo(&home, &["delete", id_str]);
    let delete_stdout = String::from_utf8_lossy(&delete_out.stdout);
    assert!(delete_out.status.success(), "memo delete failed: {:?}", delete_out);
    assert!(
        delete_stdout.contains(&format!("deleted entry #{}", id_str)),
        "unexpected delete output: {}",
        delete_stdout
    );

    // Delete again should say not found
    let delete2_out = run_memo(&home, &["delete", id_str]);
    let delete2_stdout = String::from_utf8_lossy(&delete2_out.stdout);
    assert!(
        delete2_stdout.contains("not found"),
        "expected not found: {}",
        delete2_stdout
    );
}

#[test]
fn test_inject_contains_header() {
    let home = temp_home("inject");

    run_memo(&home, &["log", "some context entry"]);

    let out = run_memo(&home, &["inject"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "memo inject failed: {:?}", out);
    assert!(
        stdout.contains("## memo context"),
        "inject output missing header: {}",
        stdout
    );
}

#[test]
fn test_stats_exits_zero() {
    let home = temp_home("stats");

    run_memo(&home, &["log", "stats entry"]);

    let out = run_memo(&home, &["stats"]);
    assert!(out.status.success(), "memo stats failed: {:?}", out);
}

#[test]
fn test_list_by_tag() {
    let home = temp_home("list_tag");

    run_memo(&home, &["log", "tagged entry", "--tag", "mytag"]);
    run_memo(&home, &["log", "untagged entry"]);

    let out = run_memo(&home, &["list", "--tag", "mytag"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "memo list --tag failed: {:?}", out);
    assert!(stdout.contains("tagged entry"), "tagged entry missing: {}", stdout);
    assert!(!stdout.contains("untagged entry"), "untagged entry shown: {}", stdout);
}

#[test]
fn test_tags_command() {
    let home = temp_home("tags_cmd");

    run_memo(&home, &["log", "a", "--tag", "alpha"]);
    run_memo(&home, &["log", "b", "--tag", "alpha"]);
    run_memo(&home, &["log", "c", "--tag", "beta"]);

    let out = run_memo(&home, &["tags"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "memo tags failed: {:?}", out);
    assert!(stdout.contains("alpha"), "alpha not listed: {}", stdout);
    assert!(stdout.contains("beta"), "beta not listed: {}", stdout);
    // alpha should appear before beta (count 2 vs 1)
    let alpha_pos = stdout.find("alpha").unwrap();
    let beta_pos = stdout.find("beta").unwrap();
    assert!(alpha_pos < beta_pos, "alpha should come before beta by count");
}

#[test]
fn test_log_stdin() {
    let home = temp_home("stdin");
    let project = home.join("project");

    let mut child = Command::new(memo_bin())
        .args(["log", "-"])
        .env("HOME", &home)
        .current_dir(&project)
        .env("GIT_DIR", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn memo");

    use std::io::Write;
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        stdin.write_all(b"stdin message\n").expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for memo");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "memo log - failed: {:?}", output);
    assert!(stdout.contains("logged: stdin message"), "unexpected: {}", stdout);
}

// ── memo capture (PostToolUse hook) ──────────────────────────────────────────

fn run_capture_with_payload(home: &PathBuf, payload: &str) -> std::process::Output {
    let project = home.join("project");
    let mut child = Command::new(memo_bin())
        .args(["capture"])
        .env("HOME", home)
        .current_dir(&project)
        .env("GIT_DIR", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn memo capture");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).expect("write payload");
    }
    child.wait_with_output().expect("wait for memo capture")
}

#[test]
fn test_capture_write_with_fn_description() {
    let home = temp_home("capture_write");
    let project = home.join("project");
    let file = project.join("src").join("auth.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();

    let content = "pub async fn handle_login(req: Request) -> Response {\n    todo!()\n}\n";
    std::fs::write(&file, content).unwrap();

    let payload = serde_json::json!({
        "tool_name": "Write",
        "tool_input": {
            "file_path": file.to_str().unwrap(),
            "content": content,
        }
    });

    let out = run_capture_with_payload(&home, &payload.to_string());
    assert!(out.status.success(), "capture failed: {:?}", out);

    // The captured entry should include the fn description
    let list_out = run_memo(&home, &["list"]);
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(
        list_stdout.contains("wrote src/auth.rs: added fn handle_login"),
        "expected smart description, got: {list_stdout}"
    );
}

#[test]
fn test_capture_edit_with_fn_description() {
    let home = temp_home("capture_edit");
    let project = home.join("project");
    let file = project.join("src").join("db.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "fn old() {}\n").unwrap();

    let payload = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {
            "file_path": file.to_str().unwrap(),
            "old_string": "fn old() {}",
            "new_string": "fn old() {}\npub fn connect_pool(config: &Config) -> Pool {\n    todo!()\n}",
        }
    });

    run_capture_with_payload(&home, &payload.to_string());

    let list_out = run_memo(&home, &["list"]);
    let stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(
        stdout.contains("edited src/db.rs: added fn connect_pool"),
        "expected smart description, got: {stdout}"
    );
}

#[test]
fn test_capture_edit_no_pattern_falls_back() {
    let home = temp_home("capture_fallback");
    let project = home.join("project");
    let file = project.join("config.toml");
    std::fs::write(&file, "port = 3000\n").unwrap();

    let payload = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {
            "file_path": file.to_str().unwrap(),
            "old_string": "port = 3000",
            "new_string": "port = 8080",
        }
    });

    run_capture_with_payload(&home, &payload.to_string());

    let list_out = run_memo(&home, &["list"]);
    let stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(
        stdout.contains("edited config.toml"),
        "expected fallback description, got: {stdout}"
    );
}

// ── memo doctor ───────────────────────────────────────────────────────────────

#[test]
fn test_doctor_after_setup() {
    let home = temp_home("doctor_setup");

    // Run setup first
    let setup_out = run_memo(&home, &["setup"]);
    assert!(setup_out.status.success(), "setup failed: {:?}", setup_out);

    let out = run_memo_with_bin_on_path(&home, &["doctor"], true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "doctor failed: {:?}", out);

    // Should report all green
    assert!(stdout.contains("All checks passed"), "expected all green, got: {stdout}");
}

#[test]
fn test_doctor_without_setup_reports_issues() {
    let home = temp_home("doctor_no_setup");

    let out = run_memo(&home, &["doctor"]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Should report issues
    assert!(
        stdout.contains("issue") || stdout.contains("missing") || stdout.contains("not found"),
        "expected issues reported, got: {stdout}"
    );
}
