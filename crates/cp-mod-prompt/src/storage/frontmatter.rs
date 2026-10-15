//! YAML frontmatter of behaviour `.md` files: one splitter shared by the
//! parser and the validator, and a serializer that always emits valid YAML.

/// The fields read from a behaviour file's frontmatter.
#[derive(serde::Deserialize, serde::Serialize, Default, Debug, PartialEq, Eq)]
pub(crate) struct Frontmatter {
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Short description.
    #[serde(default)]
    pub description: String,
}

/// Split `content` into its YAML block and body.
///
/// `None` when the content does not start with `---` or has no closing
/// `\n---` line. The body loses the newlines right after the closing marker.
pub(crate) fn split(content: &str) -> Option<(&str, &str)> {
    let after_first = content.trim_start().strip_prefix("---")?;
    let end = after_first.find("\n---")?;
    let yaml = after_first.get(..end)?;
    let body = after_first.get(end.saturating_add(4)..).unwrap_or("").trim_start_matches('\n');
    Some((yaml, body))
}

/// Read `name` / `description` from a YAML block.
///
/// Strict YAML first. Hand-written files often carry an unquoted `: ` in a
/// value (`description: paths for infra: DB, ...`), which is invalid YAML and
/// used to blank BOTH fields; those fall back to a line reader that takes
/// everything after the first `: ` as the value.
fn read_fields(yaml: &str) -> Frontmatter {
    serde_yaml::from_str(yaml).unwrap_or_else(|_| lenient_fields(yaml))
}

/// Line-based `key: value` reader for frontmatter that is not valid YAML.
fn lenient_fields(yaml: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    for line in yaml.lines() {
        let Some((key, raw)) = line.split_once(':') else { continue };
        let value = unquote(raw.trim()).to_owned();
        match key.trim() {
            "name" => fm.name = value,
            "description" => fm.description = value,
            _ => {}
        }
    }
    fm
}

/// Strip one pair of matching surrounding quotes, if any.
fn unquote(s: &str) -> &str {
    ['"', '\''].iter().find_map(|q| s.strip_prefix(*q)?.strip_suffix(*q)).unwrap_or(s)
}

/// Parse a behaviour file into `(name, description, body)`.
///
/// Without frontmatter the whole content is the body and both fields are empty.
pub(crate) fn parse(content: &str) -> (String, String, String) {
    let Some((yaml, body)) = split(content) else {
        return (String::new(), String::new(), content.to_owned());
    };
    let fm = read_fields(yaml);
    (fm.name, fm.description, body.to_owned())
}

/// Build a behaviour file. Fields are serialized by `serde_yaml`, so values
/// containing `: `, `#`, quotes or newlines are quoted and read back verbatim.
///
/// # Errors
///
/// Returns the serializer's message if the fields cannot be encoded.
pub(crate) fn format(name: &str, description: &str, body: &str) -> Result<String, String> {
    let fm = Frontmatter { name: name.to_owned(), description: description.to_owned() };
    let yaml = serde_yaml::to_string(&fm).map_err(|e| format!("Cannot encode frontmatter: {e}"))?;
    Ok(format!("---\n{yaml}---\n{body}"))
}

/// Check that `content` has well-formed frontmatter with a non-empty `name`.
///
/// # Errors
///
/// Returns `Err(reason)` if the frontmatter is missing, unclosed, invalid YAML,
/// or has no `name`.
pub(crate) fn validate(content: &str) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct Fm {
        name: Option<String>,
    }

    if !content.trim_start().starts_with("---") {
        return Err("File must start with YAML frontmatter (---)".to_owned());
    }
    let Some((yaml, _)) = split(content) else {
        return Err("Missing closing frontmatter delimiter (---)".to_owned());
    };
    let fm: Fm = serde_yaml::from_str(yaml).map_err(|e| format!("Invalid YAML frontmatter: {e}"))?;
    if fm.name.as_deref().unwrap_or("").is_empty() {
        return Err("Frontmatter must include a non-empty 'name' field".to_owned());
    }
    Ok(())
}
