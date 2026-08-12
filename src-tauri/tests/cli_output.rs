use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use tempfile::tempdir;

const SERVICE_ROLE_SENTINEL: &str = "ServiceRoleA1X";
const ORDINARY_SENTINEL: &str = "OrdinaryB2Y";
const COMMENT_SENTINEL: &str = "CommentC3Z";
const EXAMPLE_SENTINEL: &str = "ExampleD4W";
const LOCAL_SENTINEL: &str = "LocalE5V";
const ADD_SENTINEL: &str = "AddF6U";
const MALFORMED_SENTINEL: &str = "MalformedG7T";
const COPY_SENTINEL: &str = "CopyH8S";
const DESTINATION_SENTINEL: &str = "DestinationI9R";

fn run_vne<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_vne"))
        .args(args)
        .output()
        .expect("vne process should run")
}

fn write(path: &Path, content: &str) {
    fs::write(path, content).expect("fixture should be written");
}

fn assert_payload_absent(output: &Output, sentinels: &[&str]) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr should be UTF-8");

    for sentinel in sentinels {
        assert!(
            !stdout.contains(sentinel),
            "stdout exposed payload sentinel {sentinel}: {stdout}"
        );
        assert!(
            !stderr.contains(sentinel),
            "stderr exposed payload sentinel {sentinel}: {stderr}"
        );
    }

    let json: Value = serde_json::from_str(&stdout).expect("stdout should be JSON");
    let mut strings = Vec::new();
    collect_strings(&json, &mut strings);
    for sentinel in sentinels {
        assert!(
            strings.iter().all(|value| !value.contains(sentinel)),
            "parsed JSON exposed payload sentinel {sentinel}"
        );
    }

    json
}

fn collect_strings<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    match value {
        Value::String(value) => output.push(value),
        Value::Array(values) => {
            for value in values {
                collect_strings(value, output);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_strings(value, output);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[test]
fn create_auto_json_is_empty_idempotent_and_payload_free() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join(".env");
    let args = ["create".to_string(), env_file.display().to_string()];

    let created = run_vne(args.clone());
    assert_eq!(created.status.code(), Some(0));
    assert!(created.stderr.is_empty());
    let created_json: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created_json["disposition"], "created");
    assert_eq!(
        created_json["path"],
        env_file.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(created_json.as_object().unwrap().len(), 2);
    assert_eq!(fs::read(&env_file).unwrap(), b"");

    let retried = run_vne(args);
    assert_eq!(retried.status.code(), Some(0));
    assert!(retried.stderr.is_empty());
    let retried_json: Value = serde_json::from_slice(&retried.stdout).unwrap();
    assert_eq!(retried_json["disposition"], "alreadyExists");
    assert_eq!(retried_json.as_object().unwrap().len(), 2);
    assert_eq!(fs::read(&env_file).unwrap(), b"");
}

#[test]
fn create_text_reports_existing_file_without_overwriting_it() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join("worker.env");
    write(&env_file, ORDINARY_SENTINEL);

    let output = run_vne([
        "create".to_string(),
        env_file.display().to_string(),
        "--text".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "already exists {}\n",
            env_file.canonicalize().unwrap().display()
        )
    );
    assert_eq!(fs::read_to_string(env_file).unwrap(), ORDINARY_SENTINEL);
}

#[test]
fn create_rejects_invalid_names_and_missing_parents_without_stdout() {
    let dir = tempdir().unwrap();
    let invalid_name = dir.path().join("notes.txt");
    let missing_parent = dir.path().join("missing").join(".env");

    for file in [invalid_name, missing_parent] {
        let output = run_vne([
            "create".to_string(),
            file.display().to_string(),
            "--json".to_string(),
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!file.exists());
    }
}

#[test]
fn create_rejects_a_trailing_separator_or_dot_component() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join(".env");
    for target in [
        format!("{}/", env_file.display()),
        format!("{}/.", env_file.display()),
        format!("{}/..", env_file.display()),
    ] {
        let output = run_vne(["create", &target, "--json"]);

        assert_eq!(output.status.code(), Some(2), "{target}");
        assert!(output.stdout.is_empty(), "{target}");
        assert!(!output.stderr.is_empty(), "{target}");
    }
    assert!(!env_file.exists());
}

#[test]
fn create_reports_the_canonical_path_for_a_case_variant() {
    let dir = tempdir().unwrap();
    let actual_file = dir.path().join(".env.local");
    let requested_file = dir.path().join(".env.Local");
    write(&actual_file, ORDINARY_SENTINEL);
    let names_alias = requested_file.exists();

    let output = run_vne([
        "create".to_string(),
        requested_file.display().to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["disposition"],
        if names_alias {
            "alreadyExists"
        } else {
            "created"
        }
    );
    assert_eq!(
        json["path"],
        if names_alias {
            actual_file.canonicalize().unwrap()
        } else {
            requested_file.canonicalize().unwrap()
        }
        .display()
        .to_string()
    );
    assert_eq!(fs::read_to_string(actual_file).unwrap(), ORDINARY_SENTINEL);
}

#[test]
fn add_missing_file_fails_before_noninteractive_value_input() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join(".env");

    for args in [
        vec![
            "add".to_string(),
            env_file.display().to_string(),
            "TOKEN".to_string(),
            ADD_SENTINEL.to_string(),
            "--json".to_string(),
        ],
        vec![
            "add".to_string(),
            env_file.display().to_string(),
            "TOKEN".to_string(),
            "--stdin".to_string(),
            "--json".to_string(),
        ],
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vne"));
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("vne process should start");
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write as _;
            let _ = stdin.write_all(ADD_SENTINEL.as_bytes());
        }
        let output = child.wait_with_output().expect("vne process should finish");

        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("does not exist"));
        assert!(stderr.contains("vne create <file>"));
        assert!(!stderr.contains(ADD_SENTINEL));
        assert!(!env_file.exists());
    }
}

#[test]
fn add_prompt_without_terminal_does_not_offer_or_create_missing_file() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join(".env");

    let output = run_vne([
        "add".to_string(),
        env_file.display().to_string(),
        "TOKEN".to_string(),
        "--prompt".to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--prompt requires an interactive terminal"));
    assert!(stderr.contains("vne create <file>"));
    assert!(stderr.contains("--stdin"));
    assert!(!env_file.exists());
}

#[cfg(unix)]
#[test]
fn create_retry_converges_after_post_create_output_failure() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join(".env");
    let args = vec![
        "create".to_string(),
        env_file.display().to_string(),
        "--json".to_string(),
    ];

    let mut child = Command::new(env!("CARGO_BIN_EXE_vne"))
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("vne process should start");
    drop(child.stdout.take());
    let failed_output = child.wait_with_output().expect("vne process should finish");

    assert_eq!(failed_output.status.code(), Some(2));
    assert_eq!(fs::read(&env_file).unwrap(), b"");

    let retry = run_vne(args);
    assert_eq!(retry.status.code(), Some(0));
    let retry_json: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(retry_json["disposition"], "alreadyExists");
    assert_eq!(fs::read(&env_file).unwrap(), b"");
}

#[test]
fn check_json_is_payload_free_by_default() {
    let dir = tempdir().unwrap();
    let actual = dir.path().join(".env");
    let example = dir.path().join(".env.example");
    write(
        &actual,
        &format!(
            "SUPABASE_SERVICE_ROLE_KEY={SERVICE_ROLE_SENTINEL}\nPLAIN_SETTING={ORDINARY_SENTINEL} # TOKEN={COMMENT_SENTINEL}\n"
        ),
    );
    write(
        &example,
        &format!("PLAIN_SETTING={EXAMPLE_SENTINEL}\nEXAMPLE_ONLY=placeholder\n"),
    );

    let output = run_vne([
        "check".to_string(),
        actual.display().to_string(),
        "--example".to_string(),
        example.display().to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    let json = assert_payload_absent(
        &output,
        &[
            SERVICE_ROLE_SENTINEL,
            ORDINARY_SENTINEL,
            COMMENT_SENTINEL,
            EXAMPLE_SENTINEL,
        ],
    );
    assert!(json["file"]["entries"].is_array());
    assert!(json["example"]["entries"].is_array());
    assert!(json["comparison"].is_object());
}

#[test]
fn inspect_json_is_payload_free_for_every_discovered_file() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join(".env"),
        &format!("PLAIN_SETTING={ORDINARY_SENTINEL}\n"),
    );
    write(
        &dir.path().join(".env.local"),
        &format!("LOCAL_SETTING={LOCAL_SENTINEL} # TOKEN={COMMENT_SENTINEL}\n"),
    );
    write(
        &dir.path().join(".env.example"),
        &format!("PLAIN_SETTING={EXAMPLE_SENTINEL}\n"),
    );

    let output = run_vne(["inspect".to_string(), dir.path().display().to_string()]);

    assert_payload_absent(
        &output,
        &[
            ORDINARY_SENTINEL,
            LOCAL_SENTINEL,
            COMMENT_SENTINEL,
            EXAMPLE_SENTINEL,
        ],
    );
}

#[test]
fn inspect_values_mode_keeps_values_but_withholds_source_context() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join(".env"),
        &format!("PLAIN_SETTING={ORDINARY_SENTINEL} # context {COMMENT_SENTINEL}\n"),
    );
    write(
        &dir.path().join(".env.local"),
        &format!("LOCAL_SETTING={LOCAL_SENTINEL}\n"),
    );

    let output = run_vne([
        "inspect".to_string(),
        dir.path().display().to_string(),
        "--values".to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    let json = assert_payload_absent(&output, &[COMMENT_SENTINEL]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(ORDINARY_SENTINEL));
    assert!(stdout.contains(LOCAL_SENTINEL));
    assert!(json["files"].as_array().unwrap().iter().all(|file| {
        file["content"] == "[raw content withheld by output policy]"
            && file["entries"].as_array().unwrap().iter().all(|entry| {
                entry["comment"].is_null()
                    || entry["comment"] == "# [comment withheld by output policy]"
            })
    }));
}

#[test]
fn add_json_is_payload_free_without_changing_written_bytes() {
    let dir = tempdir().unwrap();
    let hidden_file = dir.path().join("hidden.env");
    let values_file = dir.path().join("values.env");
    write(
        &hidden_file,
        &format!("PORT=1420 # context {COMMENT_SENTINEL}\n"),
    );
    write(
        &values_file,
        &format!("PORT=1420 # context {COMMENT_SENTINEL}\n"),
    );

    let hidden = run_vne([
        "add".to_string(),
        hidden_file.display().to_string(),
        "NEW_SETTING".to_string(),
        ADD_SENTINEL.to_string(),
        "--json".to_string(),
    ]);
    let values = run_vne([
        "add".to_string(),
        values_file.display().to_string(),
        "NEW_SETTING".to_string(),
        ADD_SENTINEL.to_string(),
        "--values".to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(hidden.status.code(), Some(0));
    assert_eq!(values.status.code(), Some(0));
    assert_payload_absent(&hidden, &[ADD_SENTINEL, COMMENT_SENTINEL]);
    assert!(String::from_utf8_lossy(&values.stdout).contains(ADD_SENTINEL));
    let values_json = assert_payload_absent(&values, &[COMMENT_SENTINEL]);
    assert_eq!(
        values_json["content"],
        "[raw content withheld by output policy]"
    );
    assert_eq!(
        fs::read(&hidden_file).unwrap(),
        fs::read(&values_file).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn copy_json_is_payload_free_exact_and_preserves_destination_bytes_and_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let source = dir.path().join("source.env");
    let destination = dir.path().join("destination.env");
    let source_content = format!("DATABASE_URL=\"postgres://user:{COPY_SENTINEL}@host/db\"\n");
    let destination_content = "\u{feff}# keep\r\nPORT=1420\r\n";
    write(&source, &source_content);
    write(&destination, destination_content);
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o640)).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_vne"));
    let empty_path = dir.path().join("no-executables");
    fs::create_dir(&empty_path).unwrap();
    let output = command
        .args([
            "copy",
            source.to_str().unwrap(),
            "DATABASE_URL",
            destination.to_str().unwrap(),
            "--json",
        ])
        .env("PATH", &empty_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let json = assert_payload_absent(&output, &[COPY_SENTINEL]);
    assert_eq!(json["disposition"], "added");
    assert_eq!(json["key"], "DATABASE_URL");
    assert_eq!(json.as_object().unwrap().len(), 4);
    assert_eq!(
        json["sourcePath"],
        source.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        json["destinationPath"],
        destination.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), source_content);
    assert_eq!(
        fs::read_to_string(&destination).unwrap(),
        format!(
            "{destination_content}DATABASE_URL=\"postgres://user:{COPY_SENTINEL}@host/db\"\r\n"
        )
    );
    assert_eq!(
        fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn copy_errors_are_payload_free_and_leave_destination_unchanged() {
    let dir = tempdir().unwrap();
    let destination = dir.path().join("destination.env");
    let initial_destination = format!("TOKEN={DESTINATION_SENTINEL}\n");
    write(&destination, &initial_destination);

    let cases = [
        ("missing.env", "PORT=1\n".to_string(), false),
        (
            "duplicate.env",
            format!("TOKEN={COPY_SENTINEL}\nTOKEN={COPY_SENTINEL}-two\n"),
            false,
        ),
        ("malformed.env", format!("TOKEN=\"{COPY_SENTINEL}\n"), false),
        ("conflict.env", format!("TOKEN={COPY_SENTINEL}\n"), false),
    ];

    for (name, source_content, overwrite) in cases {
        let source = dir.path().join(name);
        write(&source, &source_content);
        let mut args = vec![
            "copy".to_string(),
            source.display().to_string(),
            "TOKEN".to_string(),
            destination.display().to_string(),
            "--json".to_string(),
        ];
        if overwrite {
            args.push("--overwrite".to_string());
        }

        let output = run_vne(args);

        assert_eq!(output.status.code(), Some(2), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains(COPY_SENTINEL), "{name}: {stderr}");
        assert!(!stderr.contains(DESTINATION_SENTINEL), "{name}: {stderr}");
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            initial_destination,
            "{name}"
        );
    }
}

#[test]
fn copy_overwrite_is_explicit_and_identical_retry_is_a_no_write_success() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.env");
    let destination = dir.path().join("destination.env");
    write(&source, &format!("TOKEN='source-{COPY_SENTINEL}'\n"));
    write(
        &destination,
        &format!("# keep\nTOKEN=destination-{DESTINATION_SENTINEL} # local\n"),
    );

    let refused = run_vne([
        "copy",
        source.to_str().unwrap(),
        "TOKEN",
        destination.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(refused.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(COPY_SENTINEL));
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(DESTINATION_SENTINEL));

    let overwritten = run_vne([
        "copy",
        source.to_str().unwrap(),
        "TOKEN",
        destination.to_str().unwrap(),
        "--overwrite",
        "--json",
    ]);
    assert_eq!(overwritten.status.code(), Some(0));
    let overwritten_json =
        assert_payload_absent(&overwritten, &[COPY_SENTINEL, DESTINATION_SENTINEL]);
    assert_eq!(overwritten_json["disposition"], "overwritten");
    let expected = format!("# keep\nTOKEN='source-{COPY_SENTINEL}' # local\n");
    assert_eq!(fs::read_to_string(&destination).unwrap(), expected);

    let metadata_before = fs::metadata(&destination).unwrap();
    let retried = run_vne([
        "copy",
        source.to_str().unwrap(),
        "TOKEN",
        destination.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(retried.status.code(), Some(0));
    let retried_json = assert_payload_absent(&retried, &[COPY_SENTINEL]);
    assert_eq!(retried_json["disposition"], "alreadyPresent");
    assert_eq!(fs::read_to_string(&destination).unwrap(), expected);
    let metadata_after = fs::metadata(&destination).unwrap();
    assert_eq!(
        metadata_before.modified().unwrap(),
        metadata_after.modified().unwrap()
    );
}

#[test]
fn copy_text_receipt_contains_metadata_only() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.env");
    let destination = dir.path().join("destination.env");
    write(&source, &format!("TOKEN={COPY_SENTINEL}\n"));
    write(&destination, "PORT=1420\n");

    let output = run_vne([
        "copy",
        source.to_str().unwrap(),
        "TOKEN",
        destination.to_str().unwrap(),
        "--text",
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        format!(
            "added `TOKEN` from {} to {}\n",
            source.canonicalize().unwrap().display(),
            destination.canonicalize().unwrap().display()
        )
    );
    assert!(!stdout.contains(COPY_SENTINEL));
}

#[test]
fn copy_refuses_duplicate_destination_even_with_overwrite_without_exposing_payloads() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.env");
    let destination = dir.path().join("destination.env");
    write(&source, &format!("TOKEN={COPY_SENTINEL}\n"));
    let destination_content =
        format!("TOKEN={DESTINATION_SENTINEL}-one\nTOKEN={DESTINATION_SENTINEL}-two\n");
    write(&destination, &destination_content);

    let output = run_vne([
        "copy",
        source.to_str().unwrap(),
        "TOKEN",
        destination.to_str().unwrap(),
        "--overwrite",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("lines 1, 2"));
    assert!(!stderr.contains(COPY_SENTINEL));
    assert!(!stderr.contains(DESTINATION_SENTINEL));
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        destination_content
    );
}

#[cfg(unix)]
#[test]
fn copy_retry_converges_after_post_write_output_failure() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.env");
    let destination = dir.path().join("destination.env");
    write(&source, &format!("TOKEN={COPY_SENTINEL}\n"));
    write(&destination, "PORT=1420\n");

    let args = [
        "copy",
        source.to_str().unwrap(),
        "TOKEN",
        destination.to_str().unwrap(),
        "--json",
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_vne"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let failed_output = child.wait_with_output().unwrap();
    assert_eq!(failed_output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&failed_output.stderr).contains(COPY_SENTINEL));

    let retry = run_vne(args);
    assert_eq!(retry.status.code(), Some(0));
    let retry_json = assert_payload_absent(&retry, &[COPY_SENTINEL]);
    assert_eq!(retry_json["disposition"], "alreadyPresent");
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        format!("PORT=1420\nTOKEN={COPY_SENTINEL}\n")
    );
}

#[test]
fn values_mode_keeps_ordinary_values_but_redacts_detected_payloads() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join("values.env");
    write(
        &env_file,
        &format!(
            "PLAIN_SETTING={ORDINARY_SENTINEL}\nSUPABASE_SERVICE_ROLE_KEY={SERVICE_ROLE_SENTINEL}\nPORT=1420 # TOKEN={COMMENT_SENTINEL}\n"
        ),
    );

    let output = run_vne([
        "check".to_string(),
        env_file.display().to_string(),
        "--values".to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(stdout.contains(ORDINARY_SENTINEL));
    assert_payload_absent(&output, &[SERVICE_ROLE_SENTINEL, COMMENT_SENTINEL]);
}

#[test]
fn values_mode_keeps_ordinary_values_but_withholds_source_context() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join("values-context.env");
    write(
        &env_file,
        &format!("PLAIN_SETTING={ORDINARY_SENTINEL} # context {COMMENT_SENTINEL}\n"),
    );

    let output = run_vne([
        "check".to_string(),
        env_file.display().to_string(),
        "--values".to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    let json = assert_payload_absent(&output, &[COMMENT_SENTINEL]);
    assert_eq!(json["file"]["entries"][0]["value"], ORDINARY_SENTINEL);
    assert_eq!(
        json["file"]["entries"][0]["comment"],
        "# [comment withheld by output policy]"
    );
    assert_eq!(
        json["file"]["content"],
        "[raw content withheld by output policy]"
    );
}

#[test]
fn disclosure_policy_preserves_check_exit_status() {
    let dir = tempdir().unwrap();
    let actual = dir.path().join("actual.env");
    let example = dir.path().join("example.env");
    write(&actual, &format!("EXTRA={ORDINARY_SENTINEL}\n"));
    write(&example, &format!("MISSING={EXAMPLE_SENTINEL}\n"));

    let hidden = run_vne([
        "check".to_string(),
        actual.display().to_string(),
        "--example".to_string(),
        example.display().to_string(),
        "--json".to_string(),
    ]);
    let values = run_vne([
        "check".to_string(),
        actual.display().to_string(),
        "--example".to_string(),
        example.display().to_string(),
        "--values".to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(hidden.status.code(), values.status.code());
    assert_eq!(hidden.status.code(), Some(1));
}

#[test]
fn text_output_is_independent_of_value_policy() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join("text.env");
    write(
        &env_file,
        &format!(
            "PLAIN_SETTING={ORDINARY_SENTINEL}\nSUPABASE_SERVICE_ROLE_KEY={SERVICE_ROLE_SENTINEL}\n"
        ),
    );

    let hidden = run_vne([
        "check".to_string(),
        env_file.display().to_string(),
        "--text".to_string(),
    ]);
    let values = run_vne([
        "check".to_string(),
        env_file.display().to_string(),
        "--text".to_string(),
        "--values".to_string(),
    ]);

    assert_eq!(hidden.status.code(), Some(0));
    assert_eq!(hidden.stdout, values.stdout);
    assert_eq!(hidden.stderr, values.stderr);
    let text = String::from_utf8_lossy(&hidden.stdout);
    assert!(!text.contains(ORDINARY_SENTINEL));
    assert!(!text.contains(SERVICE_ROLE_SENTINEL));
}

#[test]
fn malformed_entry_diagnostics_do_not_echo_payloads() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join("malformed.env");
    write(&env_file, &format!("BROKEN=\"{MALFORMED_SENTINEL}\n"));

    let output = run_vne([
        "check".to_string(),
        env_file.display().to_string(),
        "--json".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    let json = assert_payload_absent(&output, &[MALFORMED_SENTINEL]);
    assert!(!json["file"]["diagnostics"].as_array().unwrap().is_empty());
    assert!(!json["file"]["entries"][0]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn piped_format_warns_that_it_emits_raw_content() {
    let dir = tempdir().unwrap();
    let env_file = dir.path().join("format.env");
    write(&env_file, &format!("RAW_SETTING={ORDINARY_SENTINEL}\n"));

    let output = run_vne([
        "format".to_string(),
        env_file.display().to_string(),
        "--dry-run".to_string(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains(ORDINARY_SENTINEL));
    assert!(String::from_utf8_lossy(&output.stderr).contains("raw env content"));
}

#[test]
fn direct_read_failures_emit_no_payload_bytes() {
    let dir = tempdir().unwrap();
    let invalid_file = dir.path().join("invalid.env");
    let missing_dir = dir.path().join("missing-project");
    let mut invalid_bytes = format!("PLAIN_SETTING={ORDINARY_SENTINEL}\n").into_bytes();
    invalid_bytes.push(0xff);
    fs::write(&invalid_file, invalid_bytes).unwrap();

    let commands = [
        vec![
            "check".to_string(),
            invalid_file.display().to_string(),
            "--json".to_string(),
        ],
        vec![
            "add".to_string(),
            invalid_file.display().to_string(),
            "NEW_SETTING".to_string(),
            ADD_SENTINEL.to_string(),
            "--json".to_string(),
        ],
        vec![
            "format".to_string(),
            invalid_file.display().to_string(),
            "--dry-run".to_string(),
        ],
        vec![
            "inspect".to_string(),
            missing_dir.display().to_string(),
            "--json".to_string(),
        ],
    ];

    for args in commands {
        let output = run_vne(args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains(ORDINARY_SENTINEL));
        assert!(!stderr.contains(ADD_SENTINEL));
    }
}

#[cfg(unix)]
#[test]
fn add_post_write_output_failure_is_duplicate_safe_under_each_policy() {
    let dir = tempdir().unwrap();
    let mut initial = String::new();
    for index in 0..4_096 {
        initial.push_str(&format!("SETTING_{index:04}=ordinary-value-{index:04}\n"));
    }
    let expected = format!("{initial}NEW_SETTING={ADD_SENTINEL}\n");

    for (name, extra_args) in [("hidden.env", &[][..]), ("values.env", &["--values"][..])] {
        let env_file = dir.path().join(name);
        write(&env_file, &initial);

        let mut args = vec![
            "add".to_string(),
            env_file.display().to_string(),
            "NEW_SETTING".to_string(),
            ADD_SENTINEL.to_string(),
            "--json".to_string(),
        ];
        args.extend(extra_args.iter().map(|arg| arg.to_string()));

        let mut child = Command::new(env!("CARGO_BIN_EXE_vne"))
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("vne process should start");
        drop(child.stdout.take());
        let failed_output = child.wait_with_output().expect("vne process should finish");

        assert_eq!(failed_output.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&failed_output.stderr).contains(ADD_SENTINEL));
        assert_eq!(fs::read_to_string(&env_file).unwrap(), expected);

        let retry = run_vne(args);
        assert_eq!(retry.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&retry.stderr).contains(ADD_SENTINEL));
        assert_eq!(fs::read_to_string(&env_file).unwrap(), expected);
    }
}
