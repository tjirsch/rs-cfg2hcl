use std::fs;
use std::path::{Path, PathBuf};

/// Key prefix used to rename `variables:` blocks from Form A included files.
/// Prevents duplicate top-level key errors when both the parent and included
/// file define a `variables:` block. The variable extractor recognises this prefix.
pub const INCLUDE_VARS_PREFIX: &str = "_cfg2hcl_include_vars_";

pub fn process_includes(file_path: &Path, include_paths: &[PathBuf]) -> Result<String, Box<dyn std::error::Error>> {
    let mut counter = 0usize;
    process_includes_inner(file_path, include_paths, &mut counter)
}

fn process_includes_inner(file_path: &Path, include_paths: &[PathBuf], counter: &mut usize) -> Result<String, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read file '{}': {}", file_path.display(), e)))?;
    let mut result = Vec::new();
    let parent_dir = file_path.parent().unwrap_or(Path::new("."));

    for line in content.lines() {
        if let Some(caps) = find_include(line) {
            let (indent, key, include_file) = caps;
            let resolved_path = resolve_include_path(parent_dir, include_file, include_paths)
                .ok_or_else(|| format!("Could not resolve include file: {}", include_file))?;

            let included_content = process_includes_inner(&resolved_path, include_paths, counter)?;

            let content_indent = if key.is_some() { indent + 2 } else { indent };
            let prefix = " ".repeat(content_indent);

            if let Some(key_str) = key {
                // Form B: content is indented under a key — no top-level key conflicts possible
                result.push(format!("{}{}:", " ".repeat(indent), key_str));
                for inc_line in included_content.lines() {
                    if inc_line.trim().is_empty() {
                        result.push(String::new());
                    } else {
                        result.push(format!("{}{}", prefix, inc_line));
                    }
                }
            } else {
                // Form A: content is inserted at the same indent level as the parent.
                // Rename any top-level `variables:` block in the included file to a unique
                // internal key to prevent duplicate-key errors when both files define variables.
                let idx = *counter;
                *counter += 1;
                let renamed = rename_top_level_variables(&included_content, idx);

                // Source annotation is visible in YAML error context output
                result.push(format!("# cfg2hcl:source: {}", resolved_path.display()));
                for inc_line in renamed.lines() {
                    if inc_line.trim().is_empty() {
                        result.push(String::new());
                    } else {
                        result.push(format!("{}{}", prefix, inc_line));
                    }
                }
                result.push(format!("# cfg2hcl:source-end: {}", resolved_path.display()));
            }
        } else {
            result.push(line.to_string());
        }
    }

    Ok(result.join("\n"))
}

/// Renames the top-level `variables:` key in an included file's content to a
/// unique internal key so it can coexist with the parent file's `variables:` block.
fn rename_top_level_variables(content: &str, idx: usize) -> String {
    let new_key = format!("{}{}", INCLUDE_VARS_PREFIX, idx);
    content.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if indent == 0 && (trimmed == "variables:" || trimmed.starts_with("variables: ")) {
                let rest = &trimmed["variables".len()..];
                format!("{}{}", new_key, rest)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
