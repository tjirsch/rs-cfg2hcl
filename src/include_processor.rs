use std::fs;
use std::path::{Path, PathBuf};

pub fn process_includes(file_path: &Path, include_paths: &[PathBuf]) -> Result<String, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read file '{}': {}", file_path.display(), e)))?;
    let mut result = Vec::new();
    let parent_dir = file_path.parent().unwrap_or(Path::new("."));

    for line in content.lines() {
        if let Some(caps) = find_include(line) {
            let (indent, key, include_file) = caps;
            let resolved_path = resolve_include_path(parent_dir, include_file, include_paths)
                .ok_or_else(|| format!("Could not resolve include file: {}", include_file))?;

            let included_content = process_includes(&resolved_path, include_paths)?;

            // Calculate content indent
            let content_indent = if key.is_some() { indent + 2 } else { indent };
            let prefix = " ".repeat(content_indent);

            if let Some(key_str) = key {
                result.push(format!("{}{}:", " ".repeat(indent), key_str));
            }

            for inc_line in included_content.lines() {
                if inc_line.trim().is_empty() {
                    result.push(String::new());
                } else {
                    result.push(format!("{}{}", prefix, inc_line));
                }
            }
        } else {
            result.push(line.to_string());
        }
    }

    Ok(result.join("\n"))
}

fn find_include(line: &str) -> Option<(usize, Option<&str>, &str)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    // Form A: !include file.yaml
    if trimmed.starts_with("!include ") {
        let filename = trimmed[9..].trim().trim_matches(|c| c == '"' || c == '\'');
        return Some((indent, None, filename));
    }

    // Form B: key: !include file.yaml
    if let Some(colon_pos) = trimmed.find(':') {
        let key = &trimmed[..colon_pos].trim();
        let rest = trimmed[colon_pos + 1..].trim();
        if rest.starts_with("!include ") {
            let filename = rest[9..].trim().trim_matches(|c| c == '"' || c == '\'');
            return Some((indent, Some(key), filename));
        }
    }

    None
}

fn resolve_include_path(current_dir: &Path, include_file: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    // 1. Try relative to current file
    let rel_path = current_dir.join(include_file);
    if rel_path.exists() {
        return Some(rel_path);
    }

    // 2. Try search paths
    for path in search_paths {
        let abs_path = path.join(include_file);
        if abs_path.exists() {
            return Some(abs_path);
        }
    }

    None
}
