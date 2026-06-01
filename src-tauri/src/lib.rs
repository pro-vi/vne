use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub root: String,
    pub files: Vec<EnvFile>,
    pub comparison: Option<EnvComparison>,
    pub layer_report: EnvLayerReport,
    pub framework_profiles: Vec<FrameworkEnvProfile>,
    pub findings: Vec<EnvFinding>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvFile {
    pub path: String,
    pub name: String,
    pub discovery_reasons: Vec<String>,
    pub entries: Vec<EnvEntry>,
    pub diagnostics: Vec<String>,
    pub duplicate_keys: Vec<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub id: String,
    pub key: String,
    pub value: String,
    pub display_value: String,
    pub line_number: usize,
    pub exported: bool,
    pub quote: Option<String>,
    pub comment: Option<String>,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayerReport {
    pub ordered_files: Vec<EnvLayerFile>,
    pub overrides: Vec<EnvLayerOverride>,
    pub placeholder_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayerFile {
    pub path: String,
    pub name: String,
    pub layer_kind: String,
    pub precedence: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayerOverride {
    pub key: String,
    pub files: Vec<String>,
    pub effective_file: String,
    pub conflict: bool,
    pub redacted: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkEnvProfile {
    pub framework: String,
    pub mode: String,
    pub evidence: Vec<String>,
    pub ordered_files: Vec<FrameworkEnvFile>,
    pub missing_files: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkEnvFile {
    pub path: String,
    pub name: String,
    pub layer_kind: String,
    pub rank: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvFinding {
    pub severity: String,
    pub action_kind: String,
    pub title: String,
    pub detail: String,
    pub evidence: Vec<String>,
    pub mutation_preview: Option<String>,
    pub file_path: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLine {
    key: String,
    value: String,
    line_number: usize,
    exported: bool,
    quote: Option<char>,
    comment: Option<String>,
    value_start: usize,
    value_end: usize,
    next_start: usize,
    diagnostic: Option<String>,
}

#[tauri::command]
fn load_project(path: String) -> Result<ProjectSnapshot, String> {
    snapshot_project(Path::new(&path)).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_env_value(
    root: String,
    path: String,
    key: String,
    line_number: usize,
    value: String,
) -> Result<EnvFile, String> {
    let path = resolve_project_file(&root, &path)?;
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let updated = replace_env_value_at(&content, &key, line_number, &value)
        .ok_or_else(|| format!("Key `{key}` at line {line_number} was not found"))?;

    atomic_write_preserving_permissions(&path, &updated).map_err(|error| error.to_string())?;
    let refreshed = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    Ok(parse_env_file(&path, refreshed))
}

#[tauri::command]
fn add_env_key(root: String, path: String, key: String) -> Result<EnvFile, String> {
    let path = resolve_project_file(&root, &path)?;
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let updated = append_env_key(&content, &key, "")
        .ok_or_else(|| format!("Key `{key}` already exists or is not a valid env key"))?;

    atomic_write_preserving_permissions(&path, &updated).map_err(|error| error.to_string())?;
    let refreshed = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    Ok(parse_env_file(&path, refreshed))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load_project,
            save_env_value,
            add_env_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running vne");
}

pub fn snapshot_project(root: &Path) -> io::Result<ProjectSnapshot> {
    let root = root.canonicalize()?;
    let mut discovered = BTreeMap::<PathBuf, BTreeSet<String>>::new();

    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_env_file_name(&name) {
                add_discovered_env_file(
                    &root,
                    entry.path(),
                    format!("direct env filename `{name}`"),
                    &mut discovered,
                );
            }
        }
    }

    discover_package_json_env_files(&root, &mut discovered);
    discover_compose_env_files(&root, &mut discovered);

    let mut env_paths = discovered.into_iter().collect::<Vec<_>>();
    env_paths.sort_by_key(|(path, _)| {
        let relative = path.strip_prefix(&root).unwrap_or(path);
        (
            env_file_sort_key(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
            ),
            relative.to_string_lossy().to_string(),
        )
    });

    let files = env_paths
        .into_iter()
        .filter_map(|(path, reasons)| {
            fs::read_to_string(&path).ok().map(|content| {
                parse_env_file_with_reasons(&path, content, reasons.into_iter().collect())
            })
        })
        .collect::<Vec<_>>();

    let comparison = compare_actual_to_example(&files);
    let layer_report = build_layer_report(&files);
    let framework_profiles = build_framework_profiles(&root, &files);
    let findings = build_findings(
        comparison.as_ref(),
        &layer_report,
        &framework_profiles,
        &files,
    );

    Ok(ProjectSnapshot {
        root: root.to_string_lossy().to_string(),
        files,
        comparison,
        layer_report,
        framework_profiles,
        findings,
    })
}

pub fn parse_env_file(path: &Path, content: String) -> EnvFile {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let discovery_reasons = if is_env_file_name(&name) {
        vec![format!("direct env filename `{name}`")]
    } else {
        vec!["opened directly".to_string()]
    };

    parse_env_file_with_reasons(path, content, discovery_reasons)
}

fn parse_env_file_with_reasons(
    path: &Path,
    content: String,
    discovery_reasons: Vec<String>,
) -> EnvFile {
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicate_keys = BTreeSet::new();

    for parsed in parse_env_entries(&content) {
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
            id: entry_id(&parsed.key, parsed.line_number),
            key: parsed.key,
            value: parsed.value,
            display_value,
            line_number: parsed.line_number,
            exported: parsed.exported,
            quote: parsed.quote.map(|quote| quote.to_string()),
            comment: parsed.comment,
            shape,
            diagnostics: parsed.diagnostic.into_iter().collect(),
        });
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
        discovery_reasons,
        entries,
        diagnostics,
        duplicate_keys: duplicate_keys.into_iter().collect(),
        content,
    }
}

pub fn replace_env_value(content: &str, key: &str, value: &str) -> Option<String> {
    replace_env_entry(content, value, |parsed| parsed.key == key)
}

pub fn replace_env_value_at(
    content: &str,
    key: &str,
    line_number: usize,
    value: &str,
) -> Option<String> {
    replace_env_entry(content, value, |parsed| {
        parsed.key == key && parsed.line_number == line_number
    })
}

fn replace_env_entry<F>(content: &str, value: &str, matches_entry: F) -> Option<String>
where
    F: Fn(&ParsedLine) -> bool,
{
    let mut output = String::with_capacity(content.len() + value.len());
    let mut replaced = false;
    let mut cursor = 0;

    for parsed in parse_env_entries(content) {
        if matches_entry(&parsed) {
            let replacement = match parsed.quote {
                Some('"') => escape_double_quoted_value(value),
                Some('\'') => escape_single_quoted_value(value),
                Some(_) => escape_double_quoted_value(value),
                None if needs_quotes(value) => format!("\"{}\"", escape_double_quoted_value(value)),
                None => value.to_string(),
            };

            output.push_str(&content[cursor..parsed.value_start]);
            output.push_str(&replacement);
            cursor = parsed.value_end;
            replaced = true;
            break;
        }
    }

    output.push_str(&content[cursor..]);
    replaced.then_some(output)
}

pub fn append_env_key(content: &str, key: &str, value: &str) -> Option<String> {
    if !is_valid_env_key(key)
        || parse_env_entries(content)
            .iter()
            .any(|entry| entry.key == key)
    {
        return None;
    }

    let mut output = String::with_capacity(content.len() + key.len() + value.len() + 4);
    output.push_str(content);
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(key);
    output.push('=');
    if !value.is_empty() {
        if needs_quotes(value) {
            output.push('"');
            output.push_str(&escape_double_quoted_value(value));
            output.push('"');
        } else {
            output.push_str(value);
        }
    }
    output.push('\n');

    Some(output)
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

    let url_like = upper.ends_with("_URL") || upper.ends_with("_URI") || trimmed.contains("://");
    if upper.contains("DSN") {
        reasons.push("DSN-like key name".to_string());
        if url_like {
            reasons.push("URL-like name or value".to_string());
        }
        return shape("dsn", "Credential URL / DSN", "high", true, reasons);
    }

    if url_like && is_credential_url_key(&upper) {
        reasons.push("credential-bearing URL key name".to_string());
        reasons.push("URL-like name or value".to_string());
        return shape(
            "credential-url",
            "Credential URL / DSN",
            "high",
            true,
            reasons,
        );
    }

    if url_like && url_value_has_secretish_parts(trimmed) {
        reasons.push("credential-like URL value".to_string());
        reasons.push("URL-like name or value".to_string());
        return shape(
            "credential-url",
            "Credential URL / DSN",
            "high",
            true,
            reasons,
        );
    }

    if public_prefix {
        reasons.push("frontend-exposed prefix".to_string());
        return shape("public", "Public frontend variable", "high", false, reasons);
    }

    if url_like {
        reasons.push("URL-like name or value".to_string());
        return shape("url", "URL / DSN", "high", false, reasons);
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

fn parse_env_entries(content: &str) -> Vec<ParsedLine> {
    let mut entries = Vec::new();
    let mut offset = 0;
    let mut line_number = 1;

    while offset < content.len() {
        if let Some(parsed) = parse_env_entry_at(content, offset, line_number) {
            line_number += count_newlines(&content[offset..parsed.next_start]);
            offset = parsed.next_start;
            entries.push(parsed);
        } else {
            let next = next_line_start(content, offset);
            line_number += count_newlines(&content[offset..next]);
            offset = next;
        }
    }

    entries
}

fn parse_env_entry_at(content: &str, line_start: usize, line_number: usize) -> Option<ParsedLine> {
    let line_end = current_line_body_end(content, line_start);
    let line_body = &content[line_start..line_end];
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
    let mut comment = None;
    let value_start;
    let value_end;
    let next_start;
    let value;

    match bytes.get(index) {
        Some(b'"') | Some(b'\'') => {
            let quote_byte = bytes[index];
            quote = Some(quote_byte as char);
            index += 1;
            value_start = line_start + index;
            let mut cursor = value_start;
            let mut escaped = false;
            let mut found = false;
            while cursor < content.len() {
                let byte = content.as_bytes()[cursor];
                if byte == b'\\' && !escaped {
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
            next_start = if found {
                let comment_end = current_line_body_end(content, cursor);
                if cursor < comment_end {
                    comment = extract_inline_comment(&content[cursor + 1..comment_end]);
                }
                next_line_start(content, cursor)
            } else {
                content.len()
            };
            value = content[value_start..value_end].to_string();
            if !found {
                diagnostic = Some("quoted value is not closed".to_string());
            }
        }
        _ => {
            let local_value_start = index;
            value_start = line_start + local_value_start;
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
            while end > local_value_start && bytes[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            if comment_start < bytes.len() {
                comment = Some(line_body[comment_start..].trim().to_string());
            }
            value_end = line_start + end;
            next_start = next_line_start(content, line_start);
            value = content[value_start..value_end].to_string();
        }
    }

    Some(ParsedLine {
        key,
        value,
        line_number,
        exported,
        quote,
        comment,
        value_start,
        value_end,
        next_start,
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

fn build_layer_report(files: &[EnvFile]) -> EnvLayerReport {
    let mut ordered_files = files
        .iter()
        .filter(|file| is_layer_file(file))
        .map(|file| EnvLayerFile {
            path: file.path.clone(),
            name: file.name.clone(),
            layer_kind: layer_kind(&file.name).to_string(),
            precedence: layer_precedence(&file.name),
        })
        .collect::<Vec<_>>();

    ordered_files.sort_by_key(|file| (file.precedence, file.name.clone()));

    let mut by_key = BTreeMap::<String, Vec<(&EnvFile, &EnvEntry)>>::new();
    let mut placeholder_keys = BTreeSet::new();

    for file in files.iter().filter(|file| is_layer_file(file)) {
        for entry in &file.entries {
            by_key
                .entry(entry.key.clone())
                .or_default()
                .push((file, entry));
            if is_placeholder_value(&entry.value) {
                placeholder_keys.insert(format!("{}:{}", file.name, entry.key));
            }
        }
    }

    let mut overrides = Vec::new();
    for (key, mut entries) in by_key {
        if entries.len() < 2 {
            continue;
        }

        entries.sort_by_key(|(file, _)| (layer_precedence(&file.name), file.name.clone()));
        let unique_values = entries
            .iter()
            .map(|(_, entry)| normalized_layer_value(&entry.value))
            .collect::<BTreeSet<_>>();
        let conflict = unique_values.len() > 1;
        let effective_file = entries
            .last()
            .map(|(file, _)| file.name.clone())
            .unwrap_or_default();
        let redacted = entries
            .iter()
            .any(|(_, entry)| entry.shape.redacted_by_default);
        let files = entries
            .iter()
            .map(|(file, _)| file.name.clone())
            .collect::<Vec<_>>();
        let summary = if conflict {
            format!(
                "{} layers set different values; `{effective_file}` currently wins.",
                files.len()
            )
        } else {
            format!(
                "{} layers repeat the same value; `{effective_file}` currently wins.",
                files.len()
            )
        };

        overrides.push(EnvLayerOverride {
            key,
            files,
            effective_file,
            conflict,
            redacted,
            summary,
        });
    }

    EnvLayerReport {
        ordered_files,
        overrides,
        placeholder_keys: placeholder_keys.into_iter().collect(),
    }
}

fn build_framework_profiles(root: &Path, files: &[EnvFile]) -> Vec<FrameworkEnvProfile> {
    let Some(package_json) = read_package_json(root) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();

    if let Some(evidence) = dependency_evidence(&package_json, "next") {
        for mode in ["development", "production", "test"] {
            let desired = next_env_order(mode);
            let (ordered_files, missing_files) = framework_file_order(root, files, &desired);
            profiles.push(FrameworkEnvProfile {
                framework: "Next.js".to_string(),
                mode: mode.to_string(),
                evidence: vec![evidence.clone()],
                ordered_files,
                missing_files,
                notes: vec![
                    "Effective order is highest priority first; process.env is checked before files."
                        .to_string(),
                    "Next.js stops lookup once a key is found.".to_string(),
                    "`NEXT_PUBLIC_` keys are browser-exposed by convention.".to_string(),
                ],
            });
        }
    }

    let vite_evidence = dependency_evidence(&package_json, "vite")
        .or_else(|| dependency_evidence(&package_json, "@sveltejs/kit"));
    if let Some(evidence) = vite_evidence {
        for mode in vite_modes(&package_json) {
            let desired = vite_env_order(&mode);
            let (ordered_files, missing_files) = framework_file_order(root, files, &desired);
            profiles.push(FrameworkEnvProfile {
                framework: "Vite".to_string(),
                mode,
                evidence: vec![evidence.clone()],
                ordered_files,
                missing_files,
                notes: vec![
                    "Effective order is highest priority first; existing process env wins over env files."
                        .to_string(),
                    "Mode-specific env files override generic env files.".to_string(),
                    "`VITE_` keys are browser-exposed by convention.".to_string(),
                ],
            });
        }
    }

    profiles
}

fn read_package_json(root: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str(&content).ok()
}

fn dependency_evidence(package_json: &serde_json::Value, dependency: &str) -> Option<String> {
    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        let Some(version) = package_json
            .get(section)
            .and_then(serde_json::Value::as_object)
            .and_then(|dependencies| dependencies.get(dependency))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };

        return Some(format!("package.json {section}.{dependency} = {version}"));
    }

    None
}

fn next_env_order(mode: &str) -> Vec<String> {
    let mut names = vec![format!(".env.{mode}.local")];
    if mode != "test" {
        names.push(".env.local".to_string());
    }
    names.push(format!(".env.{mode}"));
    names.push(".env".to_string());
    names
}

fn vite_env_order(mode: &str) -> Vec<String> {
    vec![
        format!(".env.{mode}.local"),
        format!(".env.{mode}"),
        ".env.local".to_string(),
        ".env".to_string(),
    ]
}

fn vite_modes(package_json: &serde_json::Value) -> Vec<String> {
    let mut modes = BTreeSet::from(["development".to_string(), "production".to_string()]);

    if let Some(scripts) = package_json
        .get("scripts")
        .and_then(serde_json::Value::as_object)
    {
        for script in scripts.values().filter_map(serde_json::Value::as_str) {
            let tokens = shell_tokens(script);
            for (index, token) in tokens.iter().enumerate() {
                if let Some(mode) = token.strip_prefix("--mode=") {
                    if is_mode_name(mode) {
                        modes.insert(mode.to_string());
                    }
                    continue;
                }

                if token == "--mode" {
                    if let Some(mode) = tokens.get(index + 1).filter(|mode| is_mode_name(mode)) {
                        modes.insert(mode.to_string());
                    }
                }
            }
        }
    }

    modes.into_iter().collect()
}

fn framework_file_order(
    root: &Path,
    files: &[EnvFile],
    desired: &[String],
) -> (Vec<FrameworkEnvFile>, Vec<String>) {
    let mut ordered_files = Vec::new();
    let mut missing_files = Vec::new();

    for (index, name) in desired.iter().enumerate() {
        if let Some(file) = files
            .iter()
            .find(|file| file_has_root_relative_name(root, file, name))
        {
            ordered_files.push(FrameworkEnvFile {
                path: file.path.clone(),
                name: file.name.clone(),
                layer_kind: layer_kind(&file.name).to_string(),
                rank: index + 1,
            });
        } else {
            missing_files.push(name.clone());
        }
    }

    (ordered_files, missing_files)
}

fn file_has_root_relative_name(root: &Path, file: &EnvFile, name: &str) -> bool {
    Path::new(&file.path)
        .strip_prefix(root)
        .ok()
        .is_some_and(|relative| relative == Path::new(name))
}

fn is_mode_name(mode: &str) -> bool {
    !mode.is_empty()
        && mode
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn build_findings(
    comparison: Option<&EnvComparison>,
    layer_report: &EnvLayerReport,
    framework_profiles: &[FrameworkEnvProfile],
    files: &[EnvFile],
) -> Vec<EnvFinding> {
    let mut findings = Vec::new();

    if let Some(comparison) = comparison {
        for key in &comparison.missing_keys {
            findings.push(EnvFinding {
                severity: "warning".to_string(),
                action_kind: "add-missing-key".to_string(),
                title: format!("Add `{key}` to {}", file_label(&comparison.base_path)),
                detail: format!(
                    "`{key}` is documented in {} but missing from {}.",
                    file_label(&comparison.example_path),
                    file_label(&comparison.base_path)
                ),
                evidence: Vec::new(),
                mutation_preview: Some(format!(
                    "Append `{key}=` to {}.",
                    file_label(&comparison.base_path)
                )),
                file_path: Some(comparison.base_path.clone()),
                key: Some(key.clone()),
            });
        }

        for key in &comparison.extra_keys {
            findings.push(EnvFinding {
                severity: "info".to_string(),
                action_kind: "document-or-remove-key".to_string(),
                title: format!("Document or remove `{key}`"),
                detail: format!(
                    "`{key}` exists in {} but not in {}.",
                    file_label(&comparison.base_path),
                    file_label(&comparison.example_path)
                ),
                evidence: Vec::new(),
                mutation_preview: None,
                file_path: Some(comparison.base_path.clone()),
                key: Some(key.clone()),
            });
        }

        for duplicate in &comparison.duplicate_keys {
            let (file_name, key) = duplicate
                .split_once(':')
                .map(|(file_name, key)| (file_name.to_string(), key.to_string()))
                .unwrap_or_else(|| ("env file".to_string(), duplicate.clone()));
            findings.push(EnvFinding {
                severity: "warning".to_string(),
                action_kind: "resolve-duplicate-key".to_string(),
                title: format!("Resolve duplicate `{key}`"),
                detail: format!("`{file_name}` defines `{key}` more than once."),
                evidence: Vec::new(),
                mutation_preview: None,
                file_path: find_file_path_by_name(files, &file_name),
                key: Some(key),
            });
        }
    }

    for placeholder in &layer_report.placeholder_keys {
        let (file_name, key) = placeholder
            .split_once(':')
            .map(|(file_name, key)| (file_name.to_string(), key.to_string()))
            .unwrap_or_else(|| ("env file".to_string(), placeholder.clone()));
        findings.push(EnvFinding {
            severity: "warning".to_string(),
            action_kind: "replace-placeholder".to_string(),
            title: format!("Replace placeholder `{key}`"),
            detail: format!("`{file_name}` has a placeholder-like value for `{key}`."),
            evidence: Vec::new(),
            mutation_preview: None,
            file_path: find_file_path_by_name(files, &file_name),
            key: Some(key),
        });
    }

    for override_row in layer_report
        .overrides
        .iter()
        .filter(|override_row| override_row.conflict)
    {
        findings.push(EnvFinding {
            severity: "info".to_string(),
            action_kind: "review-layer-conflict".to_string(),
            title: format!("Review layered `{}`", override_row.key),
            detail: format!(
                "{} sets `{}` in multiple env layers; {} currently wins.",
                override_row.files.join(", "),
                override_row.key,
                override_row.effective_file
            ),
            evidence: framework_evidence_for_files(framework_profiles, &override_row.files),
            mutation_preview: None,
            file_path: None,
            key: Some(override_row.key.clone()),
        });
    }

    findings
}

fn framework_evidence_for_files(
    framework_profiles: &[FrameworkEnvProfile],
    file_names: &[String],
) -> Vec<String> {
    let file_names = file_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    framework_profiles
        .iter()
        .filter_map(|profile| {
            let ordered = profile
                .ordered_files
                .iter()
                .filter(|file| file_names.contains(file.name.as_str()))
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>();
            (ordered.len() >= 2).then(|| {
                format!(
                    "{} {} load order: {}",
                    profile.framework,
                    profile.mode,
                    ordered.join(" -> ")
                )
            })
        })
        .take(3)
        .collect()
}

fn find_file_path_by_name(files: &[EnvFile], file_name: &str) -> Option<String> {
    files
        .iter()
        .find(|file| file.name == file_name)
        .map(|file| file.path.clone())
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn is_layer_file(file: &EnvFile) -> bool {
    !matches!(
        file.name.as_str(),
        ".env.example" | ".env.sample" | ".env.template" | ".env.defaults"
    )
}

fn layer_kind(name: &str) -> &'static str {
    match name {
        ".env" => "base",
        ".env.local" => "local",
        ".env.development" => "development",
        ".env.production" => "production",
        ".env.test" => "test",
        ".env.development.local" => "development local",
        ".env.production.local" => "production local",
        ".env.test.local" => "test local",
        ".envrc" => "shell",
        ".flaskenv" => "Flask",
        _ if name.ends_with(".env") => "referenced",
        _ => "custom",
    }
}

fn layer_precedence(name: &str) -> usize {
    match name {
        ".env" => 10,
        ".env.development" | ".env.production" | ".env.test" => 20,
        ".env.local" => 30,
        ".env.development.local" | ".env.production.local" | ".env.test.local" => 40,
        ".envrc" | ".flaskenv" => 50,
        _ => 60,
    }
}

fn normalized_layer_value(value: &str) -> String {
    value.trim().trim_matches(['"', '\'']).to_string()
}

fn is_placeholder_value(value: &str) -> bool {
    let normalized = normalized_layer_value(value).to_ascii_lowercase();
    normalized.is_empty()
        || matches!(
            normalized.as_str(),
            "changeme" | "change-me" | "todo" | "tbd" | "replace-me" | "placeholder" | "your-value"
        )
        || normalized.starts_with("your_")
        || normalized.starts_with("your-")
        || (normalized.starts_with('<') && normalized.ends_with('>'))
}

fn key_set(file: &EnvFile) -> BTreeSet<String> {
    file.entries.iter().map(|entry| entry.key.clone()).collect()
}

fn add_discovered_env_file(
    root: &Path,
    candidate: impl AsRef<Path>,
    reason: String,
    discovered: &mut BTreeMap<PathBuf, BTreeSet<String>>,
) {
    let candidate = candidate.as_ref();
    let path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };

    let Ok(metadata) = fs::metadata(&path) else {
        return;
    };
    if !metadata.is_file() {
        return;
    }

    let Ok(canonical) = path.canonicalize() else {
        return;
    };
    if !canonical.starts_with(root) {
        return;
    }

    discovered.entry(canonical).or_default().insert(reason);
}

fn resolve_project_file(root: &str, path: &str) -> Result<PathBuf, String> {
    let root = Path::new(root)
        .canonicalize()
        .map_err(|error| format!("Project root is not accessible: {error}"))?;
    if !root.is_dir() {
        return Err("Project root is not a directory".to_string());
    }

    let path = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("Env file is not accessible: {error}"))?;
    if !path.is_file() {
        return Err("Env path is not a file".to_string());
    }
    if !path.starts_with(&root) {
        return Err("Env file is outside the active project root".to_string());
    }

    Ok(path)
}

fn discover_package_json_env_files(
    root: &Path,
    discovered: &mut BTreeMap<PathBuf, BTreeSet<String>>,
) {
    let path = root.join("package.json");
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };
    let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object) else {
        return;
    };

    for (script_name, script_value) in scripts {
        let Some(script) = script_value.as_str() else {
            continue;
        };
        for reference in extract_env_path_references(script) {
            add_discovered_env_file(
                root,
                reference,
                format!("package.json script `{script_name}`"),
                discovered,
            );
        }
    }
}

fn discover_compose_env_files(root: &Path, discovered: &mut BTreeMap<PathBuf, BTreeSet<String>>) {
    for compose_name in [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ] {
        let path = root.join(compose_name);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for reference in extract_compose_env_file_references(&content) {
            add_discovered_env_file(
                root,
                reference,
                format!("{compose_name} env_file"),
                discovered,
            );
        }
    }
}

fn extract_env_path_references(command: &str) -> Vec<String> {
    let tokens = shell_tokens(command);
    let mut references = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if let Some(path) = token.strip_prefix("--env-file=") {
            push_env_reference(&mut references, path);
            continue;
        }

        if let Some(path) = token.strip_prefix("DOTENV_CONFIG_PATH=") {
            push_env_reference(&mut references, path);
            continue;
        }

        if let Some(path) = token.strip_prefix("dotenv_config_path=") {
            push_env_reference(&mut references, path);
            continue;
        }

        if token == "--env-file" {
            if let Some(path) = tokens.get(index + 1) {
                push_env_reference(&mut references, path);
            }
            continue;
        }

        if token == "-e"
            && tokens[..index]
                .iter()
                .rev()
                .take(3)
                .any(|value| value == "dotenv")
        {
            if let Some(path) = tokens.get(index + 1) {
                push_env_reference(&mut references, path);
            }
            continue;
        }

        if matches!(token.as_str(), "-f" | "--file")
            && tokens[..index]
                .iter()
                .rev()
                .take(3)
                .any(|value| value == "env-cmd")
        {
            if let Some(path) = tokens.get(index + 1) {
                push_env_reference(&mut references, path);
            }
        }
    }

    references
}

fn extract_compose_env_file_references(content: &str) -> Vec<String> {
    let mut references = Vec::new();
    let mut in_env_file_block = false;
    let mut block_indent = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("env_file:") {
            in_env_file_block = rest.trim().is_empty();
            block_indent = indent;
            if !in_env_file_block {
                for part in rest.trim().trim_matches(['[', ']']).split(',') {
                    push_env_reference(&mut references, part);
                }
            }
            continue;
        }

        if in_env_file_block {
            if indent <= block_indent && !trimmed.starts_with('-') {
                in_env_file_block = false;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('-') {
                let candidate = rest.trim();
                if let Some(path) = candidate.strip_prefix("path:") {
                    push_env_reference(&mut references, path);
                } else {
                    push_env_reference(&mut references, candidate);
                }
            }
        }
    }

    references
}

fn push_env_reference(references: &mut Vec<String>, raw: &str) {
    let path = raw
        .trim()
        .trim_matches(['"', '\'', ',', ';'])
        .trim_start_matches("./");
    if path.is_empty() || !path.contains(".env") {
        return;
    }
    if path.contains("://") || path.starts_with("..") {
        return;
    }

    references.push(path.to_string());
}

fn shell_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;

    for character in input.chars() {
        match (quote, character) {
            (Some(active), value) if value == active => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (None, value) if matches!(value, ',' | '[' | ']' | '{' | '}') => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
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

fn entry_id(key: &str, line_number: usize) -> String {
    format!("{key}@{line_number}")
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

fn is_credential_url_key(upper: &str) -> bool {
    let url_key =
        upper.ends_with("_URL") || upper.ends_with("_URI") || upper == "URL" || upper == "URI";
    if !url_key {
        return false;
    }

    upper == "DB_URL"
        || upper == "DB_URI"
        || upper.ends_with("_DB_URL")
        || upper.ends_with("_DB_URI")
        || contains_any(
            upper,
            &[
                "DATABASE",
                "POSTGRES",
                "POSTGRESQL",
                "MYSQL",
                "MARIADB",
                "REDIS",
                "VALKEY",
                "MONGO",
                "MONGODB",
                "SQLSERVER",
                "MSSQL",
                "RABBITMQ",
                "AMQP",
                "SMTP",
                "WEBHOOK",
                "CLICKHOUSE",
                "ELASTICSEARCH",
                "OPENSEARCH",
                "KAFKA",
                "NEON",
                "PLANETSCALE",
            ],
        )
}

fn url_value_has_secretish_parts(value: &str) -> bool {
    let Some((_, after_scheme)) = value.trim().split_once("://") else {
        return false;
    };

    let authority_end = after_scheme
        .find(|character| matches!(character, '/' | '?' | '#'))
        .unwrap_or(after_scheme.len());
    if after_scheme[..authority_end].contains('@') {
        return true;
    }

    let Some(query_start) = after_scheme.find('?') else {
        return false;
    };
    let query = after_scheme[query_start + 1..]
        .split('#')
        .next()
        .unwrap_or_default();

    query.split('&').any(|parameter| {
        let name = parameter
            .split_once('=')
            .map(|(name, _)| name)
            .unwrap_or(parameter)
            .to_ascii_lowercase();
        matches!(
            name.as_str(),
            "api_key"
                | "apikey"
                | "access_key"
                | "access_token"
                | "auth"
                | "authorization"
                | "client_secret"
                | "key"
                | "password"
                | "passwd"
                | "pwd"
                | "secret"
                | "signature"
                | "sig"
                | "token"
        ) || name.ends_with("_key")
            || name.ends_with("_secret")
            || name.ends_with("_token")
    })
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn skip_spaces(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\t') {
        *index += 1;
    }
}

fn extract_inline_comment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut index = 0;
    skip_spaces(bytes, &mut index);
    (bytes.get(index) == Some(&b'#')).then(|| segment[index..].trim().to_string())
}

fn current_line_body_end(content: &str, line_start: usize) -> usize {
    let line_end = next_line_start(content, line_start);
    content[line_start..line_end]
        .trim_end_matches(['\r', '\n'])
        .len()
        + line_start
}

fn next_line_start(content: &str, offset: usize) -> usize {
    content[offset..]
        .find('\n')
        .map(|position| offset + position + 1)
        .unwrap_or(content.len())
}

fn count_newlines(value: &str) -> usize {
    value
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

fn is_key_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_key_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

fn is_valid_env_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.first().is_some_and(|first| is_key_start(*first))
        && bytes.iter().skip(1).all(|byte| is_key_continue(*byte))
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
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_single_quoted_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
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
        let content = "# db\nexport DATABASE_URL=\"postgres://localhost/app\" # local\n\nNEXT_PUBLIC_SITE=https://example.com\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());

        assert_eq!(file.content, content);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].id, "DATABASE_URL@2");
        assert_eq!(file.entries[0].key, "DATABASE_URL");
        assert_eq!(file.entries[0].value, "postgres://localhost/app");
        assert_eq!(file.entries[0].quote.as_deref(), Some("\""));
        assert_eq!(file.entries[0].comment.as_deref(), Some("# local"));
        assert!(file.entries[0].exported);
        assert_eq!(file.entries[1].shape.kind, "public");
    }

    #[test]
    fn parses_quoted_multiline_values_as_one_entry() {
        let content = "PRIVATE_KEY=\"-----BEGIN KEY-----\nabc123\n-----END KEY-----\"\nNEXT_PUBLIC_SITE=https://example.com\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());

        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].key, "PRIVATE_KEY");
        assert_eq!(
            file.entries[0].value,
            "-----BEGIN KEY-----\nabc123\n-----END KEY-----"
        );
        assert_eq!(file.entries[1].key, "NEXT_PUBLIC_SITE");
    }

    #[test]
    fn parses_unquoted_inline_comments_as_source_context() {
        let file = parse_env_file(Path::new(".env"), "PORT=1420 # dev server\n".to_string());

        assert_eq!(file.entries[0].value, "1420");
        assert_eq!(file.entries[0].comment.as_deref(), Some("# dev server"));
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
    fn replaces_duplicate_key_by_selected_line_number() {
        let content = "FEATURE_ENABLED=true\nFEATURE_ENABLED=false\nPORT=1420\n";
        let updated = replace_env_value_at(content, "FEATURE_ENABLED", 2, "maybe").unwrap();

        assert_eq!(
            updated,
            "FEATURE_ENABLED=true\nFEATURE_ENABLED=maybe\nPORT=1420\n"
        );
        assert!(replace_env_value_at(content, "FEATURE_ENABLED", 3, "maybe").is_none());
    }

    #[test]
    fn replaces_multiline_value_without_collapsing_lines() {
        let content = "PRIVATE_KEY=\"-----BEGIN KEY-----\nold\n-----END KEY-----\"\nPORT=1420\n";
        let updated = replace_env_value(
            content,
            "PRIVATE_KEY",
            "-----BEGIN KEY-----\nnew\n-----END KEY-----",
        )
        .unwrap();

        assert_eq!(
            updated,
            "PRIVATE_KEY=\"-----BEGIN KEY-----\nnew\n-----END KEY-----\"\nPORT=1420\n"
        );
        let file = parse_env_file(Path::new(".env"), updated);
        assert_eq!(
            file.entries[0].value,
            "-----BEGIN KEY-----\nnew\n-----END KEY-----"
        );
    }

    #[test]
    fn appends_missing_key_without_rewriting_existing_content() {
        let content = "# keep\nPORT=1420";
        let updated = append_env_key(content, "REDIS_URL", "").unwrap();

        assert_eq!(updated, "# keep\nPORT=1420\nREDIS_URL=\n");
        let file = parse_env_file(Path::new(".env"), updated);
        assert_eq!(file.entries[0].key, "PORT");
        assert_eq!(file.entries[1].key, "REDIS_URL");
        assert_eq!(file.entries[1].value, "");
    }

    #[test]
    fn refuses_to_append_duplicate_or_invalid_keys() {
        assert!(append_env_key("PORT=1420\n", "PORT", "").is_none());
        assert!(append_env_key("PORT=1420\n", "1INVALID", "").is_none());
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
        let redis = infer_key_shape("REDIS_URL", "redis://localhost:6379");
        assert_eq!(redis.kind, "credential-url");
        assert!(redis.redacted_by_default);
        assert_eq!(infer_key_shape("FEATURE_ENABLED", "true").kind, "bool");
        assert_eq!(infer_key_shape("PORT", "5173").kind, "port");
    }

    #[test]
    fn redacts_credential_bearing_urls_and_dsns() {
        let database = infer_key_shape("DATABASE_URL", "postgres://user:pass@localhost/app");
        assert_eq!(database.kind, "credential-url");
        assert!(database.redacted_by_default);

        let webhook = infer_key_shape("WEBHOOK_URL", "https://example.com/hook?token=abc");
        assert_eq!(webhook.kind, "credential-url");
        assert!(webhook.redacted_by_default);

        let sentry = infer_key_shape("SENTRY_DSN", "https://public@sentry.example/1");
        assert_eq!(sentry.kind, "dsn");
        assert!(sentry.redacted_by_default);

        let public_url = infer_key_shape("NEXT_PUBLIC_SITE_URL", "https://example.com");
        assert_eq!(public_url.kind, "public");
        assert!(!public_url.redacted_by_default);

        let public_with_token =
            infer_key_shape("VITE_CALLBACK_URL", "https://example.com/hook?token=abc");
        assert_eq!(public_with_token.kind, "credential-url");
        assert!(public_with_token.redacted_by_default);

        let api_url = infer_key_shape("API_BASE_URL", "https://api.example.com");
        assert_eq!(api_url.kind, "url");
        assert!(!api_url.redacted_by_default);
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

    #[test]
    fn discovers_env_files_referenced_by_common_project_config() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".env"), "DATABASE_URL=postgres://local\n").unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::create_dir_all(dir.path().join("deploy")).unwrap();
        fs::write(
            dir.path().join("config").join("worker.env"),
            "QUEUE_URL=redis://localhost\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("deploy").join("compose.env"),
            "COMPOSE_PROJECT_NAME=vne\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"worker":"node --env-file=config/worker.env worker.js"}}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("docker-compose.yml"),
            "services:\n  app:\n    env_file:\n      - deploy/compose.env\n",
        )
        .unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();
        let names = snapshot
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&".env"));
        assert!(names.contains(&"worker.env"));
        assert!(names.contains(&"compose.env"));
        assert!(snapshot
            .files
            .iter()
            .find(|file| file.name == "worker.env")
            .unwrap()
            .discovery_reasons
            .iter()
            .any(|reason| reason.contains("package.json")));
    }

    #[test]
    fn resolves_write_paths_only_inside_project_root() {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let inside_file = project.path().join(".env");
        let outside_file = outside.path().join(".env");
        fs::write(&inside_file, "PORT=1420\n").unwrap();
        fs::write(&outside_file, "PORT=1420\n").unwrap();

        assert_eq!(
            resolve_project_file(
                project.path().to_str().unwrap(),
                inside_file.to_str().unwrap()
            )
            .unwrap(),
            inside_file.canonicalize().unwrap()
        );
        assert!(resolve_project_file(
            project.path().to_str().unwrap(),
            outside_file.to_str().unwrap()
        )
        .is_err());
    }

    #[test]
    fn reports_layer_overrides_without_exposing_values() {
        let base = parse_env_file(
            Path::new(".env"),
            "DATABASE_URL=postgres://base\nJWT_SECRET=base-secret\n".to_string(),
        );
        let local = parse_env_file(
            Path::new(".env.local"),
            "DATABASE_URL=postgres://local\nJWT_SECRET=local-secret\n".to_string(),
        );
        let example = parse_env_file(
            Path::new(".env.example"),
            "DATABASE_URL=\nJWT_SECRET=<secret>\n".to_string(),
        );

        let report = build_layer_report(&[base, local, example]);

        assert_eq!(report.ordered_files.len(), 2);
        assert!(report.placeholder_keys.is_empty());
        let database = report
            .overrides
            .iter()
            .find(|override_row| override_row.key == "DATABASE_URL")
            .unwrap();
        assert_eq!(database.effective_file, ".env.local");
        assert!(database.conflict);
        assert!(database.redacted);
        assert!(!database.summary.contains("postgres://"));

        let secret = report
            .overrides
            .iter()
            .find(|override_row| override_row.key == "JWT_SECRET")
            .unwrap();
        assert!(secret.redacted);
        assert!(!secret.summary.contains("local-secret"));
    }

    #[test]
    fn builds_findings_without_exposing_values() {
        let base = parse_env_file(
            Path::new(".env"),
            "DATABASE_URL=postgres://base\nJWT_SECRET=base-secret\nEXTRA=yes\nEXTRA=no\nAPI_TOKEN=replace-me\n".to_string(),
        );
        let local = parse_env_file(
            Path::new(".env.local"),
            "DATABASE_URL=postgres://local\nJWT_SECRET=local-secret\n".to_string(),
        );
        let example = parse_env_file(
            Path::new(".env.example"),
            "DATABASE_URL=\nJWT_SECRET=\nREDIS_URL=\n".to_string(),
        );
        let files = vec![base, local, example];
        let comparison = compare_actual_to_example(&files);
        let layer_report = build_layer_report(&files);
        let framework_profiles = vec![FrameworkEnvProfile {
            framework: "Next.js".to_string(),
            mode: "development".to_string(),
            evidence: Vec::new(),
            ordered_files: vec![
                FrameworkEnvFile {
                    path: ".env.local".to_string(),
                    name: ".env.local".to_string(),
                    layer_kind: "local".to_string(),
                    rank: 2,
                },
                FrameworkEnvFile {
                    path: ".env".to_string(),
                    name: ".env".to_string(),
                    layer_kind: "base".to_string(),
                    rank: 4,
                },
            ],
            missing_files: Vec::new(),
            notes: Vec::new(),
        }];

        let findings = build_findings(
            comparison.as_ref(),
            &layer_report,
            &framework_profiles,
            &files,
        );

        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "add-missing-key"
                && finding.key.as_deref() == Some("REDIS_URL")));
        let missing_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "add-missing-key"
                    && finding.key.as_deref() == Some("REDIS_URL")
            })
            .unwrap();
        assert_eq!(
            missing_finding.mutation_preview.as_deref(),
            Some("Append `REDIS_URL=` to .env.")
        );
        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "resolve-duplicate-key"
                && finding.key.as_deref() == Some("EXTRA")));
        let duplicate_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "resolve-duplicate-key"
                    && finding.key.as_deref() == Some("EXTRA")
            })
            .unwrap();
        assert_eq!(duplicate_finding.file_path.as_deref(), Some(".env"));
        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "replace-placeholder"
                && finding.key.as_deref() == Some("API_TOKEN")));
        let placeholder_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "replace-placeholder"
                    && finding.key.as_deref() == Some("API_TOKEN")
            })
            .unwrap();
        assert_eq!(placeholder_finding.file_path.as_deref(), Some(".env"));
        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "review-layer-conflict"
                && finding.key.as_deref() == Some("JWT_SECRET")));
        let layer_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "review-layer-conflict"
                    && finding.key.as_deref() == Some("JWT_SECRET")
            })
            .unwrap();
        assert!(layer_finding
            .evidence
            .iter()
            .any(|evidence| evidence.contains("Next.js development load order")));

        let rendered = findings
            .iter()
            .map(|finding| {
                format!(
                    "{} {} {}",
                    finding.title,
                    finding.detail,
                    finding.mutation_preview.as_deref().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("base-secret"));
        assert!(!rendered.contains("local-secret"));
        assert!(!rendered.contains("postgres://"));
    }

    #[test]
    fn reports_next_framework_env_load_order() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"next":"14.2.0"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join(".env"), "DATABASE_URL=postgres://base\n").unwrap();
        fs::write(
            dir.path().join(".env.local"),
            "DATABASE_URL=postgres://local\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.development"),
            "DATABASE_URL=postgres://development\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.development.local"),
            "DATABASE_URL=postgres://development-local\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.test"),
            "DATABASE_URL=postgres://test\n",
        )
        .unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();
        let development = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Next.js" && profile.mode == "development")
            .unwrap();
        let names = development
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ".env.development.local",
                ".env.local",
                ".env.development",
                ".env"
            ]
        );
        assert!(development
            .notes
            .iter()
            .any(|note| note.contains("process.env")));

        let test_profile = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Next.js" && profile.mode == "test")
            .unwrap();
        let test_names = test_profile
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(test_names, vec![".env.test", ".env"]);
        assert!(!test_profile
            .missing_files
            .iter()
            .any(|name| name == ".env.local"));
    }

    #[test]
    fn reports_vite_framework_env_load_order_and_script_modes() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"staging":"vite build --mode staging"},"devDependencies":{"vite":"8.0.0"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join(".env"), "VITE_APP_NAME=base\n").unwrap();
        fs::write(dir.path().join(".env.local"), "VITE_APP_NAME=local\n").unwrap();
        fs::write(
            dir.path().join(".env.development"),
            "VITE_APP_NAME=development\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.development.local"),
            "VITE_APP_NAME=development-local\n",
        )
        .unwrap();
        fs::write(dir.path().join(".env.staging"), "VITE_APP_NAME=staging\n").unwrap();
        fs::write(
            dir.path().join(".env.staging.local"),
            "VITE_APP_NAME=staging-local\n",
        )
        .unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();
        let development = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Vite" && profile.mode == "development")
            .unwrap();
        let names = development
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ".env.development.local",
                ".env.development",
                ".env.local",
                ".env"
            ]
        );

        let staging = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Vite" && profile.mode == "staging")
            .unwrap();
        let staging_names = staging
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            staging_names,
            vec![".env.staging.local", ".env.staging", ".env.local", ".env"]
        );
    }
}
