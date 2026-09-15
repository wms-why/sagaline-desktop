//! Front-matter extraction from a Markdown file.
//!
//! The body is preserved verbatim — we don't interpret it. Only the YAML
//! front matter block (delimited by `---` lines at the head of the file)
//! is parsed.

use crate::error::CoreError;

/// The two halves of a Markdown file we care about.
pub struct SplitFile {
    /// `serde_yaml::Value::Null` when the file has no front matter.
    pub frontmatter: serde_yaml::Value,
    pub body: String,
}

/// Splits `text` on the first pair of `---` lines at the start of the file.
/// Anything before the second `---` is parsed as YAML; everything after it
/// is the body.
///
/// Files with no front matter return a `Null` value and the full text as
/// the body. Trailing whitespace after the closing `---` is trimmed.
pub fn split(text: &str) -> Result<SplitFile, CoreError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Normalize leading newlines so we don't mis-detect the opener.
    let stripped = text.trim_start_matches('\n');

    let Some(rest) = stripped.strip_prefix("---\n").or_else(|| {
        if stripped == "---" {
            Some("")
        } else {
            None
        }
    }) else {
        // No front matter.
        return Ok(SplitFile {
            frontmatter: serde_yaml::Value::Null,
            body: text.to_string(),
        });
    };

    let Some(close_idx) = find_line(rest, "---") else {
        return Ok(SplitFile {
            frontmatter: serde_yaml::Value::Null,
            body: text.to_string(),
        });
    };

    let yaml_src = &rest[..close_idx];
    let body_start = close_idx + "---".len();
    let body = rest[body_start..].trim_start_matches('\n').to_string();

    let frontmatter = serde_yaml::from_str(yaml_src).map_err(|source| CoreError::FrontMatter {
        path: std::path::PathBuf::from("<inline>"),
        source,
    })?;

    Ok(SplitFile { frontmatter, body })
}

/// Returns the byte offset of the next line that *equals* `marker` and is
/// either the first line or preceded by `\n`. Returns `None` if not found.
fn find_line(haystack: &str, marker: &str) -> Option<usize> {
    let mut start = 0usize;
    for line in haystack.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        if trimmed == marker {
            return Some(start);
        }
        start += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_full_document() {
        let text = "---\nid: x\ntype: character\nslug: x\n---\n# Body\n\nHello.";
        let s = split(text).unwrap();
        let yaml: serde_yaml::Value = s.frontmatter;
        assert_eq!(yaml["id"].as_str(), Some("x"));
        assert_eq!(s.body, "# Body\n\nHello.");
    }

    #[test]
    fn no_front_matter() {
        let text = "# Just a heading\n\nBody.";
        let s = split(text).unwrap();
        assert!(s.frontmatter.is_null());
        assert_eq!(s.body, text);
    }

    #[test]
    fn missing_closer_treated_as_no_front_matter() {
        let text = "---\nid: x\ntype: y\n";
        let s = split(text).unwrap();
        assert!(s.frontmatter.is_null());
        assert_eq!(s.body, text);
    }

    #[test]
    fn trims_leading_newlines() {
        let text = "\n\n---\nid: x\n---\nbody";
        let s = split(text).unwrap();
        assert_eq!(s.frontmatter["id"].as_str(), Some("x"));
        assert_eq!(s.body, "body");
    }

    #[test]
    fn strips_utf8_bom() {
        let text = "\u{feff}---\nid: x\n---\nbody";
        let s = split(text).unwrap();
        assert_eq!(s.frontmatter["id"].as_str(), Some("x"));
        assert_eq!(s.body, "body");
    }
}