/// A selector is a sequence of segments that navigate through a JSON value tree.
pub type Selector = Vec<Segment>;

/// A single segment in a selector path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Object key access, e.g. `scripts`.
    Key(String),
    /// Array index access, e.g. `[0]`.
    Index(usize),
    /// Wildcard – matches all array elements: `[*]`.
    Wildcard,
    /// Predicate filter on array elements, e.g. `[name=api]`.
    Predicate { key: String, value: String },
}

/// Parse a selector string into a [`Selector`].
///
/// # Examples
///
/// ```text
/// "scripts.test"                 → [Key("scripts"), Key("test")]
/// "jobs[0].steps[*].name"        → [Key("jobs"), Index(0), Key("steps"), Wildcard, Key("name")]
/// "jobs[id=test].timeout-minutes" → [Key("jobs"), Predicate{…}, Key("timeout-minutes")]
/// ```
pub fn parse(input: &str) -> Result<Selector, String> {
    let mut segments = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip dots between segments.
        if chars[i] == '.' {
            i += 1;
            continue;
        }

        if chars[i] == '[' {
            i += 1; // skip '['
            let start = i;
            while i < len && chars[i] != ']' {
                i += 1;
            }
            if i >= len {
                return Err("unclosed bracket in selector".to_string());
            }
            let content: String = chars[start..i].iter().collect();
            i += 1; // skip ']'

            if content == "*" {
                segments.push(Segment::Wildcard);
            } else if let Some(eq_pos) = content.find('=') {
                let key = content[..eq_pos].to_string();
                let value = content[eq_pos + 1..].to_string();
                if key.is_empty() {
                    return Err("empty predicate key".to_string());
                }
                segments.push(Segment::Predicate { key, value });
            } else if let Ok(idx) = content.parse::<usize>() {
                segments.push(Segment::Index(idx));
            } else {
                return Err(format!("invalid bracket content: {content}"));
            }
        } else {
            // Key segment: read until '.', '[', or end.
            let start = i;
            while i < len && chars[i] != '.' && chars[i] != '[' {
                i += 1;
            }
            let key: String = chars[start..i].iter().collect();
            if !key.is_empty() {
                segments.push(Segment::Key(key));
            }
        }
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_key_path() {
        let sel = parse("scripts.test").unwrap();
        assert_eq!(
            sel,
            vec![Segment::Key("scripts".into()), Segment::Key("test".into()),]
        );
    }

    #[test]
    fn parse_array_index() {
        let sel = parse("jobs[0].name").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("jobs".into()),
                Segment::Index(0),
                Segment::Key("name".into()),
            ]
        );
    }

    #[test]
    fn parse_predicate() {
        let sel = parse("jobs[id=test].timeout").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("jobs".into()),
                Segment::Predicate {
                    key: "id".into(),
                    value: "test".into(),
                },
                Segment::Key("timeout".into()),
            ]
        );
    }

    #[test]
    fn parse_wildcard() {
        let sel = parse("steps[*].name").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("steps".into()),
                Segment::Wildcard,
                Segment::Key("name".into()),
            ]
        );
    }
}
