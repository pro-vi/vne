use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub root: String,
    pub files: Vec<EnvFile>,
    pub comparison: Option<EnvComparison>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvFile {
    pub path: String,
    pub name: String,
    pub entries: Vec<EnvEntry>,
    pub diagnostics: Vec<String>,
    pub duplicate_keys: Vec<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    pub display_value: String,
    pub line_number: usize,
    pub exported: bool,
    pub quote: Option<String>,
    pub shape: KeyShape,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyShape {
    pub kind: String,
    pub label: String,
    pub confidence: String,
    pub redacted_by_default: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvComparison {
    pub base_path: String,
    pub example_path: String,
    pub missing_keys: Vec<String>,
    pub extra_keys: Vec<String>,
    pub shared_keys: Vec<String>,
    pub duplicate_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLine {
    key: String,
    value: String,
    line_number: usize,
    exported: bool,
    quote: Option<char>,
    value_start: usize,
    value_end: usize,
    diagnostic: Option<String>,
}

#[tauri::command]
fn load_project(path: String) -> Result<ProjectSnapshot, String> {
    snapshot_project(Path::new(&path)).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_env_value(path: String, key: String, value: String) -> Result<EnvFile, String> {
    let path = PathBuf::from(path);
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let updated = replace_env_value(&content, &key, &value)
        .ok_or_else(|| format!("Key `{key}` was not found"))?;

    atomic_write_preserving_permissions(&path, &updated).map_err(|error| error.to_string())?;
    let refreshed = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    Ok(parse_env_file(&path, refreshed))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![load_project, save_env_value])
        .run(tauri::generate_context!())
        .expect("error while running vne");
}

pub fn snapshot_project(root: &Path) -> io::Result<ProjectSnapshot> {
    let root = root.canonicalize()?;
    let mut env_paths = Vec::new();

    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_env_file_name(&name) {
                env_paths.push(entry.path());
            }
        }
    }

    env_paths.sort_by_key(|path| {
        env_file_sort_key(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        )
    });

    let files = env_paths
        .into_iter()
        .filter_map(|path| {
            fs::read_to_string(&path)
                .ok()
                .map(|content| parse_env_file(&path, content))
        })
        .collect::<Vec<_>>();

    let comparison = compare_actual_to_example(&files);

    Ok(ProjectSnapshot {
        root: root.to_string_lossy().to_string(),
        files,
        comparison,
    })
}

pub fn parse_env_file(path: &Path, content: String) -> EnvFile {
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicate_keys = BTreeSet::new();

    for (index, line) in content.split_inclusive('\n').enumerate() {
        if let Some(parsed) = parse_env_line(line, index + 1) {
            if !seen.insert(parsed.key.clone()) {
                duplicate_keys.insert(parsed.key.clone());
            }
            if let Some(diagnostic) = &parsed.diagnostic {
                diagnostics.push(format!("Line {}: {diagnostic}", parsed.line_number));
            }

            let shape = infer_key_shape(&parsed.key, &parsed.value);
            let display_value = if shape.redacted_by_default {
                "********".to_string()
            } else {
                parsed.value.clone()
            };

            entries.push(EnvEntry {
                key: parsed.key,
                value: parsed.value,
                display_value,
                line_number: parsed.line_number,
                exported: parsed.exported,
                quote: parsed.quote.map(|quote| quote.to_string()),
                shape,
                diagnostics: parsed.diagnostic.into_iter().collect(),
            });
        }
    }

    for key in &duplicate_keys {
        diagnostics.push(format!("Duplicate key `{key}`"));
    }

    EnvFile {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        entries,
        diagnostics,
        duplicate_keys: duplicate_keys.into_iter().collect(),
        content,
    }
}

pub fn replace_env_value(content: &str, key: &str, value: &str) -> Option<String> {
    let mut output = String::with_capacity(content.len() + value.len());
    let mut replaced = false;

    for (index, line) in content.split_inclusive('\n').enumerate() {
        if !replaced {
            if let Some(parsed) = parse_env_line(line, index + 1) {
                if parsed.key == key {
                    let replacement = match parsed.quote {
                        Some('"') => escape_double_quoted_value(value),
                        Some('\'') => escape_single_quoted_value(value),
                        Some(_) => escape_double_quoted_value(value),
                        None if needs_quotes(value) => {
                            format!("\"{}\"", escape_double_quoted_value(value))
                        }
                        None => value.to_string(),
                    };

                    output.push_str(&line[..parsed.value_start]);
                    output.push_str(&replacement);
                    output.push_str(&line[parsed.value_end..]);
                    replaced = true;
                    continue;
                }
            }
        }

        output.push_str(line);
    }

    replaced.then_some(output)
}

pub fn infer_key_shape(key: &str, value: &str) -> KeyShape {
    let upper = key.to_ascii_uppercase();
    let trimmed = value.trim();
    let mut reasons = Vec::new();

    let public_prefix = upper.starts_with("NEXT_PUBLIC_")
        || upper.starts_with("VITE_")
        || upper.starts_with("PUBLIC_");
    let provider = provider_reason(&upper);
    if let Some(provider) = provider {
        reasons.push(provider);
    }

    let secretish = contains_any(
        &upper,
        &[
            "SECRET",
            "PASSWORD",
            "TOKEN",
            "PRIVATE_KEY",
            "API_KEY",
            "ACCESS_KEY",
            "CLIENT_SECRET",
            "WEBHOOK_SECRET",
        ],
    );

    if secretish && !public_prefix {
        reasons.push("secret-like key name".to_string());
        return shape("secret", "Secret", "high", true, reasons);
    }

    if trimmed.starts_with("-----BEGIN ") {
        reasons.push("PEM block marker".to_string());
        return shape("pem", "PEM / certificate", "high", true, reasons);
    }

    if public_prefix {
        reasons.push("frontend-exposed prefix".to_string());
        return shape("public", "Public frontend variable", "high", false, reasons);
    }

    if upper.ends_with("_URL") || upper.ends_with("_URI") || trimmed.contains("://") {
        reasons.push("URL-like name or value".to_string());
        return shape("url", "URL / DSN", "high", false, reasons);
    }

    if upper.contains("DSN") {
        reasons.push("DSN-like key name".to_string());
        return shape("dsn", "DSN", "high", true, reasons);
    }

    if upper.ends_with("_PORT") || key == "PORT" || trimmed.parse::<u16>().is_ok() {
        reasons.push("port-like key or numeric port value".to_string());
        return shape("port", "Port", "medium", false, reasons);
    }

    if matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "true" | "false" | "1" | "0" | "yes" | "no"
    ) {
        reasons.push("boolean-like value".to_string());
        return shape("bool", "Boolean", "medium", false, reasons);
    }

    if trimmed.parse::<i64>().is_ok() {
        reasons.push("integer value".to_string());
        return shape("int", "Integer", "medium", false, reasons);
    }

    if upper.ends_with("_TIMEOUT")
        || upper.ends_with("_TTL")
        || upper.ends_with("_INTERVAL")
        || looks_like_duration(trimmed)
    {
        reasons.push("duration-like key or value".to_string());
        return shape("duration", "Duration", "medium", false, reasons);
    }

    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        reasons.push("JSON-looking value".to_string());
        return shape("json", "JSON", "medium", false, reasons);
    }

    if looks_like_uuid(trimmed) {
        reasons.push("UUID-looking value".to_string());
        return shape("uuid", "UUID", "medium", false, reasons);
    }

    if trimmed.contains('@') && trimmed.contains('.') {
        reasons.push("email-looking value".to_string());
        return shape("email", "Email", "medium", false, reasons);
    }

    if upper.ends_with("_PATH")
        || upper.ends_with("_DIR")
        || upper.ends_with("_FILE")
        || trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("~/")
    {
        reasons.push("path-like key or value".to_string());
        return shape("path", "File path", "medium", false, reasons);
    }

    if trimmed.contains(',') {
        reasons.push("comma-separated value".to_string());
        return shape("list", "List", "medium", false, reasons);
    }

    if looks_like_base64(trimmed) {
        reasons.push("base64-looking value".to_string());
        return shape("base64", "Base64", "low", true, reasons);
    }

    if upper.ends_with("_HOST") || upper.ends_with("_HOSTNAME") || key == "HOST" {
        reasons.push("host-like key name".to_string());
        return shape("host", "Host", "medium", false, reasons);
    }

    shape("text", "Text", "low", false, reasons)
}

fn parse_env_line(line: &str, line_number: usize) -> Option<ParsedLine> {
    let line_body = line.trim_end_matches(['\r', '\n']);
    let bytes = line_body.as_bytes();
    let mut index = 0;

    skip_spaces(bytes, &mut index);
    if index >= bytes.len() || bytes[index] == b'#' {
        return None;
    }

    let mut exported = false;
    if bytes[index..].starts_with(b"export")
        && bytes.get(index + 6).is_some_and(u8::is_ascii_whitespace)
    {
        exported = true;
        index += 6;
        skip_spaces(bytes, &mut index);
    }

    let key_start = index;
    if index >= bytes.len() || !is_key_start(bytes[index]) {
        return None;
    }
    index += 1;
    while index < bytes.len() && is_key_continue(bytes[index]) {
        index += 1;
    }
    let key_end = index;
    let key = line_body[key_start..key_end].to_string();

    skip_spaces(bytes, &mut index);
    if bytes.get(index) != Some(&b'=') {
        return None;
    }
    index += 1;
    skip_spaces(bytes, &mut index);

    let mut diagnostic = None;
    let mut quote = None;
    let value_start;
    let value_end;
    let value;

    match bytes.get(index) {
        Some(b'"') | Some(b'\'') => {
            let quote_byte = bytes[index];
            quote = Some(quote_byte as char);
            index += 1;
            value_start = index;
            let mut cursor = index;
            let mut escaped = false;
            let mut found = false;
            while cursor < bytes.len() {
                let byte = bytes[cursor];
                if quote_byte == b'"' && byte == b'\\' && !escaped {
                    escaped = true;
                    cursor += 1;
                    continue;
                }
                if byte == quote_byte && !escaped {
                    found = true;
                    break;
                }
                escaped = false;
                cursor += 1;
            }
            value_end = cursor;
            value = line_body[value_start..value_end].to_string();
            if !found {
                diagnostic = Some("quoted value is not closed on this line".to_string());
            }
        }
        _ => {
            value_start = index;
            let mut cursor = index;
            let mut comment_start = bytes.len();
            while cursor < bytes.len() {
                if bytes[cursor] == b'#'
                    && (cursor == index || bytes[cursor.saturating_sub(1)].is_ascii_whitespace())
                {
                    comment_start = cursor;
                    break;
                }
                cursor += 1;
            }
            let mut end = comment_start;
            while end > value_start && bytes[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            value_end = end;
            value = line_body[value_start..value_end].to_string();
        }
    }

    Some(ParsedLine {
        key,
        value,
        line_number,
        exported,
        quote,
        value_start,
        value_end,
        diagnostic,
    })
}

fn compare_actual_to_example(files: &[EnvFile]) -> Option<EnvComparison> {
    let base = files.iter().find(|file| file.name == ".env")?;
    let example = files.iter().find(|file| {
        matches!(
            file.name.as_str(),
            ".env.example" | ".env.sample" | ".env.template" | ".env.defaults"
        )
    })?;

    let base_keys = key_set(base);
    let example_keys = key_set(example);

    let missing_keys = example_keys.difference(&base_keys).cloned().collect();
    let extra_keys = base_keys.difference(&example_keys).cloned().collect();
    let shared_keys = base_keys.intersection(&example_keys).cloned().collect();
    let duplicate_keys = files
        .iter()
        .flat_map(|file| {
            file.duplicate_keys
                .iter()
                .map(|key| format!("{}:{key}", file.name))
        })
        .collect();

    Some(EnvComparison {
        base_path: base.path.clone(),
        example_path: example.path.clone(),
        missing_keys,
        extra_keys,
        shared_keys,
        duplicate_keys,
    })
}

fn key_set(file: &EnvFile) -> BTreeSet<String> {
    file.entries.iter().map(|entry| entry.key.clone()).collect()
}

fn is_env_file_name(name: &str) -> bool {
    name == ".env"
        || name.starts_with(".env.")
        || matches!(name, ".envrc" | ".flaskenv")
        || name.ends_with(".env")
}

fn env_file_sort_key(name: &str) -> (u8, String) {
    let priority = match name {
        ".env" => 0,
        ".env.local" => 1,
        ".env.development" => 2,
        ".env.production" => 3,
        ".env.test" => 4,
        ".env.example" | ".env.sample" | ".env.template" | ".env.defaults" => 5,
        _ => 9,
    };
    (priority, name.to_string())
}

fn atomic_write_preserving_permissions(path: &Path, content: &str) -> io::Result<()> {
    let permissions = fs::metadata(path)
        .map(|metadata| metadata.permissions())
        .ok();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("env");
    let tmp_path = path.with_file_name(format!(".{file_name}.vne.tmp"));

    fs::write(&tmp_path, content)?;
    if let Some(permissions) = permissions {
        fs::set_permissions(&tmp_path, permissions)?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn shape(
    kind: &str,
    label: &str,
    confidence: &str,
    redacted_by_default: bool,
    reasons: Vec<String>,
) -> KeyShape {
    KeyShape {
        kind: kind.to_string(),
        label: label.to_string(),
        confidence: confidence.to_string(),
        redacted_by_default,
        reasons,
    }
}

fn provider_reason(upper: &str) -> Option<String> {
    let provider = if upper.starts_with("AWS_") {
        "AWS"
    } else if upper.starts_with("GCP_") || upper.starts_with("GOOGLE_") {
        "Google Cloud"
    } else if upper.starts_with("AZURE_") {
        "Azure"
    } else if upper.starts_with("STRIPE_") {
        "Stripe"
    } else if upper.starts_with("SUPABASE_") {
        "Supabase"
    } else if upper.starts_with("CLERK_") {
        "Clerk"
    } else if upper.starts_with("SENTRY_") {
        "Sentry"
    } else if upper.starts_with("OPENAI_") {
        "OpenAI"
    } else if upper.starts_with("ANTHROPIC_") {
        "Anthropic"
    } else {
        return None;
    };

    Some(format!("{provider} convention"))
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn skip_spaces(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\t') {
        *index += 1;
    }
}

fn is_key_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_key_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value.contains(char::is_whitespace)
        || value.contains('#')
        || value.contains('"')
        || value.contains('\'')
        || value.contains('\n')
}

fn escape_double_quoted_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn escape_single_quoted_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
}

fn looks_like_duration(value: &str) -> bool {
    let split_at = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    if split_at == 0 || split_at == value.len() {
        return false;
    }
    matches!(&value[split_at..], "ms" | "s" | "m" | "h" | "d")
}

fn looks_like_uuid(value: &str) -> bool {
    let parts = value.split('-').map(str::len).collect::<Vec<_>>();
    parts == [8, 4, 4, 4, 12]
        && value
            .chars()
            .all(|character| character == '-' || character.is_ascii_hexdigit())
}

fn looks_like_base64(value: &str) -> bool {
    value.len() >= 16
        && value.len() % 4 == 0
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '_' | '-' | '=')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_entries_without_destroying_file_structure() {
        let content = "# db\nexport DATABASE_URL=\"postgres://localhost/app\"\n\nNEXT_PUBLIC_SITE=https://example.com\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());

        assert_eq!(file.content, content);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].key, "DATABASE_URL");
        assert_eq!(file.entries[0].value, "postgres://localhost/app");
        assert_eq!(file.entries[0].quote.as_deref(), Some("\""));
        assert!(file.entries[0].exported);
        assert_eq!(file.entries[1].shape.kind, "public");
    }

    #[test]
    fn replaces_one_value_while_preserving_comments_order_and_quote_style() {
        let content = "# keep\nDATABASE_URL=\"postgres://old/app\" # local\nPORT=3000\n";
        let updated = replace_env_value(content, "DATABASE_URL", "postgres://new/app").unwrap();

        assert_eq!(
            updated,
            "# keep\nDATABASE_URL=\"postgres://new/app\" # local\nPORT=3000\n"
        );
    }

    #[test]
    fn escapes_replacement_inside_single_quotes() {
        let content = "GREETING='hello'\n";
        let updated = replace_env_value(content, "GREETING", "can't stop").unwrap();

        assert_eq!(updated, "GREETING='can\\'t stop'\n");
    }

    #[test]
    fn infers_common_key_shapes_and_redaction() {
        assert_eq!(infer_key_shape("OPENAI_API_KEY", "sk-test").kind, "secret");
        assert!(infer_key_shape("OPENAI_API_KEY", "sk-test").redacted_by_default);
        assert_eq!(
            infer_key_shape("REDIS_URL", "redis://localhost:6379").kind,
            "url"
        );
        assert_eq!(infer_key_shape("FEATURE_ENABLED", "true").kind, "bool");
        assert_eq!(infer_key_shape("PORT", "5173").kind, "port");
    }

    #[test]
    fn compares_env_against_example_and_names_duplicates() {
        let actual = parse_env_file(
            Path::new(".env"),
            "DATABASE_URL=postgres://local\nEXTRA=yes\nEXTRA=no\n".to_string(),
        );
        let example = parse_env_file(
            Path::new(".env.example"),
            "DATABASE_URL=\nREDIS_URL=\n".to_string(),
        );
        let comparison = compare_actual_to_example(&[actual, example]).unwrap();

        assert_eq!(comparison.missing_keys, vec!["REDIS_URL"]);
        assert_eq!(comparison.extra_keys, vec!["EXTRA"]);
        assert_eq!(comparison.duplicate_keys, vec![".env:EXTRA"]);
    }

    #[test]
    fn scans_common_env_files_in_a_project() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".env"), "DATABASE_URL=postgres://local\n").unwrap();
        fs::write(
            dir.path().join(".env.example"),
            "DATABASE_URL=\nREDIS_URL=\n",
        )
        .unwrap();
        fs::write(dir.path().join("notes.txt"), "ignore").unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();

        assert_eq!(snapshot.files.len(), 2);
        assert_eq!(snapshot.comparison.unwrap().missing_keys, vec!["REDIS_URL"]);
    }
}
