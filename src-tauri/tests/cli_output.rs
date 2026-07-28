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
