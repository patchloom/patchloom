pub(crate) mod doc {
    use crate::selector;
    use std::path::Path;

    #[derive(Debug, Clone, Copy)]
    pub(crate) enum FileFormat {
        Json,
        Yaml,
        Toml,
    }

    pub(crate) fn detect_format(path: &str) -> anyhow::Result<FileFormat> {
        match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some("json") => Ok(FileFormat::Json),
            Some("yaml" | "yml") => Ok(FileFormat::Yaml),
            Some("toml") => Ok(FileFormat::Toml),
            Some(ext) => anyhow::bail!("unsupported file extension: .{ext}"),
            None => anyhow::bail!("file has no extension"),
        }
    }

    pub(crate) fn serialize_value(
        value: &serde_json::Value,
        format: &FileFormat,
    ) -> anyhow::Result<String> {
        match format {
            FileFormat::Json => {
                let mut s = serde_json::to_string_pretty(value)?;
                s.push('\n');
                Ok(s)
            }
            FileFormat::Yaml => Ok(serde_yaml_ng::to_string(value)?),
            FileFormat::Toml => {
                let s = toml_edit::ser::to_string_pretty(value)
                    .map_err(|e| anyhow::anyhow!("TOML serialization error: {e}"))?;
                Ok(s)
            }
        }
    }

    pub(crate) fn parse_doc(
        content: &str,
        format: &FileFormat,
    ) -> anyhow::Result<serde_json::Value> {
        match format {
            FileFormat::Json => Ok(serde_json::from_str(content)?),
            FileFormat::Yaml => Ok(serde_yaml_ng::from_str(content)?),
            FileFormat::Toml => Ok(toml_edit::de::from_str(content)?),
        }
    }

    pub(crate) fn navigate_mut<'a>(
        root: &'a mut serde_json::Value,
        segments: &[selector::Segment],
        create: bool,
    ) -> anyhow::Result<&'a mut serde_json::Value> {
        let mut current = root;
        for seg in segments {
            current = match seg {
                selector::Segment::Key(k) => {
                    if create {
                        let needs_create = match current.as_object() {
                            Some(obj) => !obj.contains_key(k.as_str()),
                            None => false,
                        };
                        if needs_create {
                            current
                                .as_object_mut()
                                .ok_or_else(|| anyhow::anyhow!("not an object at key '{k}'"))?
                                .insert(
                                    k.clone(),
                                    serde_json::Value::Object(serde_json::Map::new()),
                                );
                        }
                    }
                    current
                        .get_mut(k.as_str())
                        .ok_or_else(|| anyhow::anyhow!("key not found: {k}"))?
                }
                selector::Segment::Index(i) => current
                    .get_mut(*i)
                    .ok_or_else(|| anyhow::anyhow!("index out of bounds: {i}"))?,
                _ => anyhow::bail!("wildcard/predicate not supported in write navigation"),
            };
        }
        Ok(current)
    }

    /// Set a value at the location described by `segments`.  Navigates to the
    /// parent (creating intermediate keys when needed) and inserts the value at
    /// the final Key or Index segment.
    pub(crate) fn set_at_path(
        root: &mut serde_json::Value,
        segments: &[selector::Segment],
        value: serde_json::Value,
    ) -> anyhow::Result<()> {
        let last = segments
            .last()
            .ok_or_else(|| anyhow::anyhow!("empty selector"))?;
        let parent_path = &segments[..segments.len() - 1];
        let parent = navigate_mut(root, parent_path, true)?;

        match last {
            selector::Segment::Key(k) => {
                parent
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                    .insert(k.clone(), value);
            }
            selector::Segment::Index(i) => {
                let arr = parent
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                if *i < arr.len() {
                    arr[*i] = value;
                } else {
                    anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
                }
            }
            _ => anyhow::bail!("cannot set at wildcard/predicate"),
        }
        Ok(())
    }

    const MAX_MERGE_DEPTH: usize = 128;

    pub(crate) fn deep_merge(base: &mut serde_json::Value, other: &serde_json::Value) {
        deep_merge_inner(base, other, 0);
    }

    fn deep_merge_inner(base: &mut serde_json::Value, other: &serde_json::Value, depth: usize) {
        if depth >= MAX_MERGE_DEPTH {
            *base = other.clone();
            return;
        }
        if let (Some(base_map), Some(other_map)) = (base.as_object_mut(), other.as_object()) {
            for (key, value) in other_map {
                let entry = base_map
                    .entry(key.clone())
                    .or_insert(serde_json::Value::Null);
                deep_merge_inner(entry, value, depth + 1);
            }
        } else {
            *base = other.clone();
        }
    }

    pub(crate) fn update_matching(
        value: &mut serde_json::Value,
        segments: &[selector::Segment],
        new_val: &serde_json::Value,
    ) -> usize {
        if segments.is_empty() {
            *value = new_val.clone();
            return 1;
        }
        let first = &segments[0];
        let rest = &segments[1..];
        match first {
            selector::Segment::Key(k) => {
                if let Some(child) = value.get_mut(k.as_str()) {
                    update_matching(child, rest, new_val)
                } else {
                    0
                }
            }
            selector::Segment::Index(i) => {
                if let Some(child) = value.get_mut(*i) {
                    update_matching(child, rest, new_val)
                } else {
                    0
                }
            }
            selector::Segment::Wildcard => {
                let mut count = 0;
                if let Some(arr) = value.as_array_mut() {
                    for item in arr.iter_mut() {
                        count += update_matching(item, rest, new_val);
                    }
                }
                count
            }
            selector::Segment::Predicate {
                key,
                value: pred_val,
            } => {
                let mut count = 0;
                if let Some(arr) = value.as_array_mut() {
                    for item in arr.iter_mut() {
                        let matches = {
                            item.get(key.as_str()).is_some_and(|field| match field {
                                serde_json::Value::String(s) => s == pred_val,
                                serde_json::Value::Number(n) => n.to_string() == *pred_val,
                                serde_json::Value::Bool(b) => b.to_string() == *pred_val,
                                _ => false,
                            })
                        };
                        if matches {
                            count += update_matching(item, rest, new_val);
                        }
                    }
                }
                count
            }
        }
    }
}

pub(crate) mod replace {
    use regex::Regex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ReplaceModeError {
        MissingMode,
        BothInsertModes,
        ToWithInsert,
    }

    pub(crate) fn validate_replace_mode(
        has_to: bool,
        has_insert_before: bool,
        has_insert_after: bool,
    ) -> Result<(), ReplaceModeError> {
        match (has_to, has_insert_before, has_insert_after) {
            (false, false, false) => Err(ReplaceModeError::MissingMode),
            (_, true, true) => Err(ReplaceModeError::BothInsertModes),
            (true, true, false) | (true, false, true) => Err(ReplaceModeError::ToWithInsert),
            _ => Ok(()),
        }
    }

    pub(crate) fn replacement_text(
        from: &str,
        to: &Option<String>,
        insert_before: &Option<String>,
        insert_after: &Option<String>,
        use_match_anchor: bool,
    ) -> String {
        let anchor = if use_match_anchor { "${0}" } else { from };

        if let Some(text) = insert_before {
            return format!("{text}{anchor}");
        }

        if let Some(text) = insert_after {
            return format!("{anchor}{text}");
        }

        to.clone().unwrap_or_default()
    }

    fn expand_regex_replacement(caps: &regex::Captures<'_>, replacement: &str) -> String {
        let mut expanded = String::new();
        caps.expand(replacement, &mut expanded);
        expanded
    }

    pub(crate) fn replace_content(
        content: &str,
        from: &str,
        to: &str,
        compiled_re: Option<&Regex>,
        nth: Option<usize>,
    ) -> (String, usize) {
        match (nth, compiled_re) {
            (Some(n), Some(re)) => {
                let mut count = 0usize;
                let mut result = String::with_capacity(content.len());
                for m in re.find_iter(content) {
                    count += 1;
                    if count != n {
                        continue;
                    }

                    result.push_str(&content[..m.start()]);
                    if let Some(caps) = re.captures(&content[m.start()..]) {
                        let replacement = expand_regex_replacement(&caps, to);
                        result.push_str(&replacement);
                    }
                    result.push_str(&content[m.end()..]);
                    return (result, 1);
                }
                (content.to_owned(), 0)
            }
            (Some(n), None) => {
                let mut count = 0usize;
                let mut result = String::with_capacity(content.len());
                for (start, _) in content.match_indices(from) {
                    count += 1;
                    if count != n {
                        continue;
                    }

                    result.push_str(&content[..start]);
                    result.push_str(to);
                    result.push_str(&content[start + from.len()..]);
                    return (result, 1);
                }
                (content.to_owned(), 0)
            }
            (None, Some(re)) => {
                let mut count = 0usize;
                let replaced = re
                    .replace_all(content, |caps: &regex::Captures| {
                        count += 1;
                        expand_regex_replacement(caps, to)
                    })
                    .to_string();
                if count == 0 {
                    return (content.to_owned(), 0);
                }
                (replaced, count)
            }
            (None, None) => {
                let count = content.matches(from).count();
                if count == 0 {
                    return (content.to_owned(), 0);
                }
                let replaced = content.replace(from, to);
                (replaced, count)
            }
        }
    }
}

pub(crate) mod md {
    use std::collections::HashSet;

    #[derive(Debug, Clone)]
    pub(crate) struct HeadingInfo {
        pub level: usize,
        pub text: String,
        pub line_start: usize,
        pub line_end: usize,
    }

    pub(crate) fn parse_headings(content: &str) -> Vec<HeadingInfo> {
        let lines: Vec<&str> = content.lines().collect();
        let mut headings = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            if !line.starts_with('#') {
                continue;
            }
            let hashes = line.bytes().take_while(|&b| b == b'#').count();
            if hashes > 6 || hashes >= line.len() {
                continue;
            }
            if line.as_bytes()[hashes] != b' ' {
                continue;
            }
            headings.push(HeadingInfo {
                level: hashes,
                text: line[hashes + 1..].to_string(),
                line_start: idx,
                line_end: 0,
            });
        }

        let total = lines.len();
        for i in 0..headings.len() {
            let lvl = headings[i].level;
            let mut end = total;
            for h in headings.iter().skip(i + 1) {
                if h.level <= lvl {
                    end = h.line_start;
                    break;
                }
            }
            headings[i].line_end = end;
        }

        headings
    }

    fn line_byte_starts(content: &str) -> Vec<usize> {
        let mut starts = vec![0];
        for (i, b) in content.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        starts
    }

    fn normalize_heading_query(heading: &str) -> &str {
        let t = heading.trim();
        let n = t.bytes().take_while(|&b| b == b'#').count();
        if n > 0 && t.len() > n && t.as_bytes()[n] == b' ' {
            t[n + 1..].trim()
        } else {
            t
        }
    }

    pub(crate) fn find_section(content: &str, heading: &str) -> Option<(usize, usize)> {
        let headings = parse_headings(content);
        let offsets = line_byte_starts(content);
        let query = normalize_heading_query(heading);

        for h in &headings {
            if h.text.trim() == query {
                let body_start = if h.line_start + 1 < offsets.len() {
                    offsets[h.line_start + 1]
                } else {
                    content.len()
                };
                let body_end = if h.line_end < offsets.len() {
                    offsets[h.line_end]
                } else {
                    content.len()
                };
                return Some((body_start, body_end));
            }
        }
        None
    }

    pub(crate) fn replace_section_in(
        content: &str,
        heading: &str,
        replacement: &str,
    ) -> Option<String> {
        let (body_start, body_end) = find_section(content, heading)?;
        let mut out = String::with_capacity(content.len());
        out.push_str(&content[..body_start]);
        if !replacement.is_empty() {
            out.push_str(replacement);
            if !replacement.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push_str(&content[body_end..]);
        Some(out)
    }

    pub(crate) fn insert_after_heading_in(
        content: &str,
        heading: &str,
        insertion: &str,
    ) -> Option<String> {
        let (body_start, _) = find_section(content, heading)?;
        let mut out = String::with_capacity(content.len() + insertion.len());
        out.push_str(&content[..body_start]);
        out.push_str(insertion);
        if !insertion.is_empty() && !insertion.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&content[body_start..]);
        Some(out)
    }

    pub(crate) fn insert_before_heading_in(
        content: &str,
        heading: &str,
        insertion: &str,
    ) -> Option<String> {
        let headings = parse_headings(content);
        let offsets = line_byte_starts(content);
        let query = normalize_heading_query(heading);

        for h in &headings {
            if h.text.trim() == query {
                let heading_start = offsets[h.line_start];
                let mut out = String::with_capacity(content.len() + insertion.len());
                out.push_str(&content[..heading_start]);
                if !insertion.is_empty() {
                    out.push_str(insertion);
                    if !insertion.ends_with('\n') {
                        out.push('\n');
                    }
                    if !out.ends_with("\n\n") {
                        out.push('\n');
                    }
                }
                out.push_str(&content[heading_start..]);
                return Some(out);
            }
        }
        None
    }

    pub(crate) fn upsert_bullet_in(content: &str, heading: &str, bullet: &str) -> Option<String> {
        let (body_start, body_end) = find_section(content, heading)?;
        let body = &content[body_start..body_end];

        let trimmed = bullet.trim();
        let normalized = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            trimmed.to_string()
        } else {
            format!("- {trimmed}")
        };

        for line in body.lines() {
            if line.trim() == normalized {
                return Some(content.to_string());
            }
        }

        let mut out = String::with_capacity(content.len() + normalized.len() + 2);
        out.push_str(&content[..body_end]);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&normalized);
        out.push('\n');
        out.push_str(&content[body_end..]);
        Some(out)
    }

    pub(crate) fn dedupe_headings_in(content: &str) -> (String, Vec<String>) {
        let headings = parse_headings(content);
        let offsets = line_byte_starts(content);
        let mut seen: HashSet<(usize, String)> = HashSet::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut removed: Vec<String> = Vec::new();

        for h in &headings {
            let key = (h.level, h.text.trim().to_string());
            if seen.contains(&key) {
                let start = offsets[h.line_start];
                let end = if h.line_end < offsets.len() {
                    offsets[h.line_end]
                } else {
                    content.len()
                };
                ranges.push((start, end));
                removed.push(format!("{} {}", "#".repeat(h.level), h.text));
            } else {
                seen.insert(key);
            }
        }

        let mut out = String::with_capacity(content.len());
        let mut pos = 0;
        for (start, end) in &ranges {
            if *start < pos {
                continue;
            }
            out.push_str(&content[pos..*start]);
            pos = *end;
        }
        out.push_str(&content[pos..]);

        (out, removed)
    }

    fn is_table_row(line: &str) -> bool {
        let t = line.trim();
        t.len() > 1 && t.starts_with('|') && t.ends_with('|')
    }

    fn is_separator_row(line: &str) -> bool {
        let t = line.trim();
        if t.len() < 3 || !t.starts_with('|') || !t.ends_with('|') {
            return false;
        }
        t[1..t.len() - 1]
            .chars()
            .all(|c| matches!(c, '-' | ':' | '|' | ' '))
    }

    pub(crate) fn table_append_in(
        content: &str,
        body_start: usize,
        body_end: usize,
        row: &str,
    ) -> Option<String> {
        let body = &content[body_start..body_end];
        let mut last_data_end: Option<usize> = None;
        let mut in_table = false;
        let mut pos = body_start;

        for line in body.lines() {
            let line_byte_end = pos + line.len();
            let next_pos = if content.as_bytes().get(line_byte_end) == Some(&b'\r')
                && content.as_bytes().get(line_byte_end + 1) == Some(&b'\n')
            {
                line_byte_end + 2
            } else if content.as_bytes().get(line_byte_end) == Some(&b'\n') {
                line_byte_end + 1
            } else {
                line_byte_end
            };

            if is_table_row(line) {
                in_table = true;
                if !is_separator_row(line) {
                    last_data_end = Some(next_pos);
                }
            } else if in_table {
                break;
            }

            pos = next_pos;
        }

        let insert_pos = last_data_end?;

        let mut out = String::with_capacity(content.len() + row.len() + 2);
        out.push_str(&content[..insert_pos]);
        out.push_str(row);
        if !row.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&content[insert_pos..]);
        Some(out)
    }

    pub(crate) fn table_append_for_tx(content: &str, heading: &str, row: &str) -> Option<String> {
        let (body_start, body_end) = find_section(content, heading)?;
        table_append_in(content, body_start, body_end, row)
    }
}

pub(crate) mod patch {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum PatchLine {
        Context(String),
        Remove(String),
        Add(String),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Hunk {
        pub(crate) old_start: usize,
        pub(crate) old_count: usize,
        pub(crate) new_start: usize,
        pub(crate) new_count: usize,
        pub(crate) lines: Vec<PatchLine>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct PatchFile {
        pub(crate) path: String,
        pub(crate) hunks: Vec<Hunk>,
    }

    pub(crate) fn parse_patch(input: &str) -> Result<Vec<PatchFile>, String> {
        let lines: Vec<&str> = input.lines().collect();
        let mut files: Vec<PatchFile> = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            if !lines[i].starts_with("--- ") {
                i += 1;
                continue;
            }

            if i + 1 >= lines.len() || !lines[i + 1].starts_with("+++ ") {
                return Err(format!("expected +++ line after --- at line {}", i + 1));
            }

            let path = parse_file_path(lines[i + 1]);
            i += 2;

            let mut hunks: Vec<Hunk> = Vec::new();
            while i < lines.len() && !lines[i].starts_with("--- ") {
                if lines[i].starts_with("@@ ") {
                    let hunk = parse_hunk_header(lines[i])?;
                    let mut hunk_lines: Vec<PatchLine> = Vec::new();
                    i += 1;

                    while i < lines.len()
                        && !lines[i].starts_with("@@ ")
                        && !lines[i].starts_with("--- ")
                    {
                        let line = lines[i];
                        if let Some(rest) = line.strip_prefix('+') {
                            hunk_lines.push(PatchLine::Add(rest.to_string()));
                        } else if let Some(rest) = line.strip_prefix('-') {
                            hunk_lines.push(PatchLine::Remove(rest.to_string()));
                        } else if let Some(rest) = line.strip_prefix(' ') {
                            hunk_lines.push(PatchLine::Context(rest.to_string()));
                        } else if line == "\\ No newline at end of file" {
                        } else {
                            hunk_lines.push(PatchLine::Context(line.to_string()));
                        }
                        i += 1;
                    }

                    hunks.push(Hunk {
                        old_start: hunk.old_start,
                        old_count: hunk.old_count,
                        new_start: hunk.new_start,
                        new_count: hunk.new_count,
                        lines: hunk_lines,
                    });
                } else {
                    i += 1;
                }
            }

            if hunks.is_empty() {
                return Err(format!("no hunks found for file {path}"));
            }

            files.push(PatchFile { path, hunks });
        }

        if files.is_empty() {
            return Err("no files found in patch".to_string());
        }

        Ok(files)
    }

    fn parse_file_path(line: &str) -> String {
        let raw = line
            .strip_prefix("+++ ")
            .or_else(|| line.strip_prefix("--- "))
            .unwrap_or(line);

        raw.strip_prefix("b/")
            .or_else(|| raw.strip_prefix("a/"))
            .unwrap_or(raw)
            .to_string()
    }

    fn parse_hunk_header(line: &str) -> Result<Hunk, String> {
        let trimmed = line
            .strip_prefix("@@ ")
            .ok_or_else(|| format!("invalid hunk header: {line}"))?;

        let end = trimmed
            .find(" @@")
            .ok_or_else(|| format!("invalid hunk header (no closing @@): {line}"))?;
        let range_part = &trimmed[..end];

        let parts: Vec<&str> = range_part.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!("invalid hunk header ranges: {line}"));
        }

        let (old_start, old_count) = parse_range(parts[0].strip_prefix('-').unwrap_or(parts[0]))?;
        let (new_start, new_count) = parse_range(parts[1].strip_prefix('+').unwrap_or(parts[1]))?;

        Ok(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines: Vec::new(),
        })
    }

    fn parse_range(s: &str) -> Result<(usize, usize), String> {
        if let Some((a, b)) = s.split_once(',') {
            let start = a
                .parse::<usize>()
                .map_err(|e| format!("bad range start '{a}': {e}"))?;
            let count = b
                .parse::<usize>()
                .map_err(|e| format!("bad range count '{b}': {e}"))?;
            Ok((start, count))
        } else {
            let start = s
                .parse::<usize>()
                .map_err(|e| format!("bad range '{s}': {e}"))?;
            Ok((start, 1))
        }
    }

    const FUZZ_RANGE: usize = 3;

    pub(crate) fn apply_hunks(original: &str, hunks: &[Hunk]) -> Result<String, String> {
        let mut src_lines: Vec<String> = original.lines().map(String::from).collect();
        let had_final_newline = original.ends_with('\n') || original.is_empty();
        let mut offset: isize = 0;

        for (hunk_idx, hunk) in hunks.iter().enumerate() {
            let expected: isize = if hunk.old_start == 0 {
                0
            } else {
                hunk.old_start as isize - 1 + offset
            };

            let old_lines: Vec<String> = hunk
                .lines
                .iter()
                .filter_map(|pl| match pl {
                    PatchLine::Context(s) => Some(s.clone()),
                    PatchLine::Remove(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();

            let src_refs: Vec<&str> = src_lines.iter().map(|s| s.as_str()).collect();
            let old_refs: Vec<&str> = old_lines.iter().map(|s| s.as_str()).collect();

            let pos = find_match(&src_refs, &old_refs, expected, FUZZ_RANGE).ok_or_else(|| {
                let snippet = old_lines
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "hunk {} failed: stale context near line {} — expected:\n{}",
                    hunk_idx + 1,
                    hunk.old_start,
                    snippet,
                )
            })?;

            let new_lines: Vec<String> = hunk
                .lines
                .iter()
                .filter_map(|pl| match pl {
                    PatchLine::Context(s) => Some(s.clone()),
                    PatchLine::Add(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();

            let old_len = old_lines.len();
            let new_len = new_lines.len();
            src_lines.splice(pos..pos + old_len, new_lines);
            offset += new_len as isize - old_len as isize;
        }

        Ok(join_lines(&src_lines, had_final_newline))
    }

    fn join_lines(lines: &[String], final_newline: bool) -> String {
        if lines.is_empty() {
            return String::new();
        }
        let mut out = lines.join("\n");
        if final_newline {
            out.push('\n');
        }
        out
    }

    fn find_match(
        haystack: &[&str],
        needle: &[&str],
        expected: isize,
        fuzz: usize,
    ) -> Option<usize> {
        if needle.is_empty() {
            let pos = expected.max(0) as usize;
            return Some(pos.min(haystack.len()));
        }

        for delta in 0..=fuzz {
            for &sign in &[1isize, -1isize] {
                let candidate = expected + (delta as isize) * sign;
                if candidate < 0 {
                    continue;
                }
                let pos = candidate as usize;
                if pos + needle.len() > haystack.len() {
                    continue;
                }
                if haystack[pos..pos + needle.len()] == *needle {
                    return Some(pos);
                }
            }
        }

        None
    }

    pub(crate) fn apply_patch_with_loader<F>(
        diff_text: &str,
        mut load_original: F,
    ) -> anyhow::Result<Vec<(String, String)>>
    where
        F: FnMut(&str) -> anyhow::Result<String>,
    {
        let patch_files =
            parse_patch(diff_text).map_err(|msg| anyhow::anyhow!("patch parse error: {msg}"))?;
        let mut results = Vec::new();
        for pf in &patch_files {
            let original = load_original(&pf.path)?;
            let patched = apply_hunks(&original, &pf.hunks)
                .map_err(|msg| anyhow::anyhow!("patch apply: {} -- {msg}", pf.path))?;
            results.push((pf.path.clone(), patched));
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    // ── doc module tests ──────────────────────────────────────────────
    mod doc_tests {
        use crate::ops::doc::*;
        use serde_json::json;

        #[test]
        fn detect_format_json() {
            assert!(matches!(
                detect_format("config.json").unwrap(),
                FileFormat::Json
            ));
        }

        #[test]
        fn detect_format_yaml() {
            assert!(matches!(
                detect_format("config.yaml").unwrap(),
                FileFormat::Yaml
            ));
            assert!(matches!(
                detect_format("config.yml").unwrap(),
                FileFormat::Yaml
            ));
        }

        #[test]
        fn detect_format_toml() {
            assert!(matches!(
                detect_format("Cargo.toml").unwrap(),
                FileFormat::Toml
            ));
        }

        #[test]
        fn detect_format_unsupported() {
            assert!(detect_format("readme.txt").is_err());
        }

        #[test]
        fn detect_format_no_extension() {
            assert!(detect_format("Makefile").is_err());
        }

        #[test]
        fn parse_and_serialize_json_roundtrip() {
            let input = "{\n  \"a\": 1\n}\n";
            let val = parse_doc(input, &FileFormat::Json).unwrap();
            assert_eq!(val, json!({"a": 1}));
            let out = serialize_value(&val, &FileFormat::Json).unwrap();
            assert_eq!(out, input);
        }

        #[test]
        fn parse_and_serialize_yaml_roundtrip() {
            let input = "a: 1\n";
            let val = parse_doc(input, &FileFormat::Yaml).unwrap();
            assert_eq!(val, json!({"a": 1}));
            let out = serialize_value(&val, &FileFormat::Yaml).unwrap();
            assert_eq!(out, input);
        }

        #[test]
        fn parse_and_serialize_toml_roundtrip() {
            let input = "a = 1\n";
            let val = parse_doc(input, &FileFormat::Toml).unwrap();
            assert_eq!(val, json!({"a": 1}));
            // TOML pretty serialization may differ slightly; just ensure it parses back
            let out = serialize_value(&val, &FileFormat::Toml).unwrap();
            let reparsed = parse_doc(&out, &FileFormat::Toml).unwrap();
            assert_eq!(reparsed, json!({"a": 1}));
        }

        #[test]
        fn navigate_mut_existing_key() {
            let mut val = json!({"a": {"b": 42}});
            let seg = crate::selector::parse("a.b").unwrap();
            let found = navigate_mut(&mut val, &seg, false).unwrap();
            assert_eq!(*found, json!(42));
        }

        #[test]
        fn navigate_mut_missing_key_no_create() {
            let mut val = json!({"a": 1});
            let seg = crate::selector::parse("b").unwrap();
            assert!(navigate_mut(&mut val, &seg, false).is_err());
        }

        #[test]
        fn navigate_mut_create_missing_key() {
            let mut val = json!({"a": 1});
            let seg = crate::selector::parse("b.c").unwrap();
            let found = navigate_mut(&mut val, &seg, true).unwrap();
            // created as empty object, then descended into "c" which was also created
            assert!(found.is_object());
        }

        #[test]
        fn navigate_mut_array_index() {
            let mut val = json!({"items": [10, 20, 30]});
            let seg = crate::selector::parse("items[1]").unwrap();
            let found = navigate_mut(&mut val, &seg, false).unwrap();
            assert_eq!(*found, json!(20));
        }

        #[test]
        fn navigate_mut_index_out_of_bounds() {
            let mut val = json!({"items": [10]});
            let seg = crate::selector::parse("items[5]").unwrap();
            assert!(navigate_mut(&mut val, &seg, false).is_err());
        }

        #[test]
        fn deep_merge_objects() {
            let mut base = json!({"a": 1, "b": {"c": 2}});
            let other = json!({"b": {"d": 3}, "e": 4});
            deep_merge(&mut base, &other);
            assert_eq!(base, json!({"a": 1, "b": {"c": 2, "d": 3}, "e": 4}));
        }

        #[test]
        fn deep_merge_overwrites_non_object() {
            let mut base = json!({"a": "string"});
            let other = json!({"a": {"nested": true}});
            deep_merge(&mut base, &other);
            assert_eq!(base, json!({"a": {"nested": true}}));
        }

        #[test]
        fn deep_merge_depth_limit() {
            // Build a deeply nested structure beyond MAX_MERGE_DEPTH (128)
            let mut deep_val = json!("leaf");
            for _ in 0..130 {
                deep_val = json!({"n": deep_val});
            }
            let mut base = json!({});
            deep_merge(&mut base, &deep_val);
            // Should not panic; at depth 128 it clones instead of recursing
            assert!(base.is_object());
        }

        #[test]
        fn update_matching_by_key() {
            let mut val = json!({"a": {"b": "old"}});
            let seg = crate::selector::parse("a.b").unwrap();
            let count = update_matching(&mut val, &seg, &json!("new"));
            assert_eq!(count, 1);
            assert_eq!(val, json!({"a": {"b": "new"}}));
        }

        #[test]
        fn update_matching_wildcard() {
            let mut val = json!({"items": [{"v": 1}, {"v": 2}]});
            let seg = crate::selector::parse("items[*].v").unwrap();
            let count = update_matching(&mut val, &seg, &json!(99));
            assert_eq!(count, 2);
            assert_eq!(val, json!({"items": [{"v": 99}, {"v": 99}]}));
        }

        #[test]
        fn update_matching_predicate() {
            let mut val = json!({"items": [
                {"name": "a", "v": 1},
                {"name": "b", "v": 2}
            ]});
            let seg = crate::selector::parse("items[name=b].v").unwrap();
            let count = update_matching(&mut val, &seg, &json!(42));
            assert_eq!(count, 1);
            assert_eq!(val["items"][1]["v"], json!(42));
            // First item unchanged
            assert_eq!(val["items"][0]["v"], json!(1));
        }

        #[test]
        fn update_matching_missing_key_returns_zero() {
            let mut val = json!({"a": 1});
            let seg = crate::selector::parse("b.c").unwrap();
            let count = update_matching(&mut val, &seg, &json!("x"));
            assert_eq!(count, 0);
        }
    }

    // ── replace module tests ──────────────────────────────────────────
    mod replace_tests {
        use crate::ops::replace::*;

        #[test]
        fn validate_mode_missing() {
            assert_eq!(
                validate_replace_mode(false, false, false),
                Err(ReplaceModeError::MissingMode)
            );
        }

        #[test]
        fn validate_mode_both_inserts() {
            assert_eq!(
                validate_replace_mode(false, true, true),
                Err(ReplaceModeError::BothInsertModes)
            );
        }

        #[test]
        fn validate_mode_to_with_insert() {
            assert_eq!(
                validate_replace_mode(true, true, false),
                Err(ReplaceModeError::ToWithInsert)
            );
            assert_eq!(
                validate_replace_mode(true, false, true),
                Err(ReplaceModeError::ToWithInsert)
            );
        }

        #[test]
        fn validate_mode_valid_to_only() {
            assert!(validate_replace_mode(true, false, false).is_ok());
        }

        #[test]
        fn validate_mode_valid_insert_before_only() {
            assert!(validate_replace_mode(false, true, false).is_ok());
        }

        #[test]
        fn validate_mode_valid_insert_after_only() {
            assert!(validate_replace_mode(false, false, true).is_ok());
        }

        #[test]
        fn replacement_text_with_to() {
            let result = replacement_text("from", &Some("to".into()), &None, &None, false);
            assert_eq!(result, "to");
        }

        #[test]
        fn replacement_text_insert_before_literal() {
            let result =
                replacement_text("original", &None, &Some("PREFIX\n".into()), &None, false);
            assert_eq!(result, "PREFIX\noriginal");
        }

        #[test]
        fn replacement_text_insert_after_literal() {
            let result =
                replacement_text("original", &None, &None, &Some("\nSUFFIX".into()), false);
            assert_eq!(result, "original\nSUFFIX");
        }

        #[test]
        fn replacement_text_insert_before_regex_anchor() {
            let result = replacement_text("ignored", &None, &Some("PREFIX\n".into()), &None, true);
            assert_eq!(result, "PREFIX\n${0}");
        }

        #[test]
        fn replacement_text_insert_after_regex_anchor() {
            let result = replacement_text("ignored", &None, &None, &Some("\nSUFFIX".into()), true);
            assert_eq!(result, "${0}\nSUFFIX");
        }

        #[test]
        fn replace_content_literal_all() {
            let (out, count) = replace_content("aXbXc", "X", "Y", None, None);
            assert_eq!(out, "aYbYc");
            assert_eq!(count, 2);
        }

        #[test]
        fn replace_content_literal_no_match() {
            let (out, count) = replace_content("hello", "zzz", "y", None, None);
            assert_eq!(out, "hello");
            assert_eq!(count, 0);
        }

        #[test]
        fn replace_content_literal_nth() {
            let (out, count) = replace_content("aXbXcX", "X", "Y", None, Some(2));
            assert_eq!(out, "aXbYcX");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_literal_nth_out_of_range() {
            let (out, count) = replace_content("aXb", "X", "Y", None, Some(5));
            assert_eq!(out, "aXb");
            assert_eq!(count, 0);
        }

        #[test]
        fn replace_content_regex_all() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("a1b22c333", "unused", "N", Some(&re), None);
            assert_eq!(out, "aNbNcN");
            assert_eq!(count, 3);
        }

        #[test]
        fn replace_content_regex_nth() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("a1b22c333", "unused", "N", Some(&re), Some(2));
            assert_eq!(out, "a1bNc333");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_regex_capture_group() {
            let re = regex::Regex::new(r"(\w+)@(\w+)").unwrap();
            let (out, count) = replace_content("user@host", "unused", "$2=$1", Some(&re), None);
            assert_eq!(out, "host=user");
            assert_eq!(count, 1);
        }
    }

    // ── md module tests ───────────────────────────────────────────────
    mod md_tests {
        use crate::ops::md::*;

        #[test]
        fn parse_headings_basic() {
            let content = "# H1\ntext\n## H2\nmore\n# H1b\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 3);
            assert_eq!(headings[0].level, 1);
            assert_eq!(headings[0].text, "H1");
            assert_eq!(headings[1].level, 2);
            assert_eq!(headings[1].text, "H2");
            assert_eq!(headings[2].level, 1);
            assert_eq!(headings[2].text, "H1b");
        }

        #[test]
        fn parse_headings_section_boundaries() {
            // ## B (level 2) does NOT end # A (level 1); only same-or-higher level ends it
            let content = "# A\nline1\nline2\n## B\nline3\n";
            let headings = parse_headings(content);
            assert_eq!(headings[0].line_start, 0);
            assert_eq!(headings[0].line_end, 5); // # A owns everything (no same-level heading)
            assert_eq!(headings[1].line_start, 3);
            assert_eq!(headings[1].line_end, 5); // ## B to end of content

            // Two same-level headings: second ends first
            let content2 = "# A\nbody\n# B\nmore\n";
            let h2 = parse_headings(content2);
            assert_eq!(h2[0].line_end, 2); // # A ends at # B
            assert_eq!(h2[1].line_end, 4); // # B to end
        }

        #[test]
        fn parse_headings_ignores_invalid() {
            let content = "#nospace\n##also\n# Valid\n###### Six\n####### Seven\n";
            let headings = parse_headings(content);
            // Only "# Valid" and "###### Six" are valid (Seven > 6 levels)
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Valid");
            assert_eq!(headings[1].text, "Six");
        }

        #[test]
        fn find_section_returns_body_bytes() {
            // ## Next is deeper than # Title, so it's part of the section body
            let content = "# Title\nBody line 1\nBody line 2\n## Next\n";
            let (start, end) = find_section(content, "Title").unwrap();
            let body = &content[start..end];
            assert_eq!(body, "Body line 1\nBody line 2\n## Next\n");

            // Same-level heading ends the section
            let content2 = "# Title\nBody\n# Other\nKeep\n";
            let (s2, e2) = find_section(content2, "Title").unwrap();
            assert_eq!(&content2[s2..e2], "Body\n");
        }

        #[test]
        fn find_section_with_hashes_in_query() {
            let content = "## API\nsome text\n";
            let result = find_section(content, "## API");
            assert!(result.is_some());
        }

        #[test]
        fn find_section_missing() {
            let content = "# Title\nBody\n";
            assert!(find_section(content, "Nonexistent").is_none());
        }

        #[test]
        fn replace_section_basic() {
            // Use same-level heading so section boundary is clear
            let content = "# Title\nOld body\n# Next\nKeep\n";
            let result = replace_section_in(content, "Title", "New body").unwrap();
            assert_eq!(result, "# Title\nNew body\n# Next\nKeep\n");
        }

        #[test]
        fn replace_section_empty_replacement() {
            let content = "# Title\nOld body\n# Next\nKeep\n";
            let result = replace_section_in(content, "Title", "").unwrap();
            assert_eq!(result, "# Title\n# Next\nKeep\n");
        }

        #[test]
        fn replace_section_missing_heading() {
            let content = "# Title\nBody\n";
            assert!(replace_section_in(content, "Missing", "x").is_none());
        }

        #[test]
        fn insert_after_heading() {
            let content = "# Title\nExisting\n";
            let result = insert_after_heading_in(content, "Title", "Inserted\n").unwrap();
            assert_eq!(result, "# Title\nInserted\nExisting\n");
        }

        #[test]
        fn insert_before_heading() {
            let content = "# First\nBody\n## Second\nMore\n";
            let result = insert_before_heading_in(content, "Second", "Inserted").unwrap();
            assert!(result.contains("Inserted\n\n## Second"));
        }

        #[test]
        fn upsert_bullet_adds_new() {
            let content = "# List\n- item1\n";
            let result = upsert_bullet_in(content, "List", "- item2").unwrap();
            assert!(result.contains("- item1\n- item2\n"));
        }

        #[test]
        fn upsert_bullet_dedup_existing() {
            let content = "# List\n- item1\n";
            let result = upsert_bullet_in(content, "List", "- item1").unwrap();
            // Should return content unchanged (no duplicate)
            assert_eq!(result, content);
        }

        #[test]
        fn upsert_bullet_auto_prefix() {
            let content = "# List\n- a\n";
            let result = upsert_bullet_in(content, "List", "new item").unwrap();
            assert!(result.contains("- new item\n"));
        }

        #[test]
        fn dedupe_headings_removes_duplicate() {
            let content = "# Title\nFirst\n# Title\nSecond\n";
            let (result, removed) = dedupe_headings_in(content);
            assert_eq!(removed, vec!["# Title"]);
            // First occurrence kept, second removed
            assert!(result.contains("First"));
            assert!(!result.contains("Second"));
        }

        #[test]
        fn dedupe_headings_no_duplicates() {
            let content = "# A\n## B\n# C\n";
            let (result, removed) = dedupe_headings_in(content);
            assert!(removed.is_empty());
            assert_eq!(result, content);
        }

        #[test]
        fn table_append_basic() {
            let content = "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n## Next\n";
            let (start, end) = find_section(content, "API").unwrap();
            let result = table_append_in(content, start, end, "| b | 2 |").unwrap();
            assert!(result.contains("| a | 1 |\n| b | 2 |\n## Next"));
        }

        #[test]
        fn table_append_no_table() {
            let content = "# API\nJust text\n";
            let (start, end) = find_section(content, "API").unwrap();
            assert!(table_append_in(content, start, end, "| b | 2 |").is_none());
        }

        #[test]
        fn table_append_for_tx_basic() {
            let content = "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n";
            let result = table_append_for_tx(content, "API", "| b | 2 |").unwrap();
            assert!(result.contains("| a | 1 |\n| b | 2 |\n"));
        }
    }

    // ── patch module tests ────────────────────────────────────────────
    mod patch_tests {
        use crate::ops::patch::*;

        #[test]
        fn parse_patch_single_file() {
            let diff = "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-line2
+LINE2
 line3
";
            let files = parse_patch(diff).unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "hello.txt");
            assert_eq!(files[0].hunks.len(), 1);
            assert_eq!(files[0].hunks[0].old_start, 1);
            assert_eq!(files[0].hunks[0].old_count, 3);
        }

        #[test]
        fn parse_patch_multiple_files() {
            let diff = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-old
+new
--- a/b.txt
+++ b/b.txt
@@ -1,1 +1,1 @@
-foo
+bar
";
            let files = parse_patch(diff).unwrap();
            assert_eq!(files.len(), 2);
            assert_eq!(files[0].path, "a.txt");
            assert_eq!(files[1].path, "b.txt");
        }

        #[test]
        fn parse_patch_no_files() {
            let diff = "just some text\n";
            assert!(parse_patch(diff).is_err());
        }

        #[test]
        fn parse_patch_no_hunks() {
            let diff = "--- a/f.txt\n+++ b/f.txt\n";
            assert!(parse_patch(diff).is_err());
        }

        #[test]
        fn apply_hunks_simple_replacement() {
            let original = "line1\nline2\nline3\n";
            let hunks = vec![Hunk {
                old_start: 2,
                old_count: 1,
                new_start: 2,
                new_count: 1,
                lines: vec![
                    PatchLine::Context("line1".into()),
                    PatchLine::Remove("line2".into()),
                    PatchLine::Add("LINE2".into()),
                    PatchLine::Context("line3".into()),
                ],
            }];
            let result = apply_hunks(original, &hunks).unwrap();
            assert_eq!(result, "line1\nLINE2\nline3\n");
        }

        #[test]
        fn apply_hunks_addition() {
            let original = "a\nb\n";
            let hunks = vec![Hunk {
                old_start: 1,
                old_count: 2,
                new_start: 1,
                new_count: 3,
                lines: vec![
                    PatchLine::Context("a".into()),
                    PatchLine::Add("inserted".into()),
                    PatchLine::Context("b".into()),
                ],
            }];
            let result = apply_hunks(original, &hunks).unwrap();
            assert_eq!(result, "a\ninserted\nb\n");
        }

        #[test]
        fn apply_hunks_deletion() {
            let original = "a\nremove_me\nb\n";
            let hunks = vec![Hunk {
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 2,
                lines: vec![
                    PatchLine::Context("a".into()),
                    PatchLine::Remove("remove_me".into()),
                    PatchLine::Context("b".into()),
                ],
            }];
            let result = apply_hunks(original, &hunks).unwrap();
            assert_eq!(result, "a\nb\n");
        }

        #[test]
        fn apply_hunks_stale_context_fails() {
            let original = "a\nb\nc\n";
            let hunks = vec![Hunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                lines: vec![
                    PatchLine::Remove("wrong_context".into()),
                    PatchLine::Add("x".into()),
                ],
            }];
            assert!(apply_hunks(original, &hunks).is_err());
        }

        #[test]
        fn apply_hunks_fuzz_match() {
            // The hunk header says line 2, but the actual match is at line 3
            // (1 line off). Should still apply within FUZZ_RANGE=3.
            let original = "a\nb\nc\nd\n";
            let hunks = vec![Hunk {
                old_start: 2,
                old_count: 1,
                new_start: 2,
                new_count: 1,
                lines: vec![PatchLine::Remove("c".into()), PatchLine::Add("C".into())],
            }];
            let result = apply_hunks(original, &hunks).unwrap();
            assert_eq!(result, "a\nb\nC\nd\n");
        }

        #[test]
        fn apply_patch_with_loader_basic() {
            let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 hello
-world
+WORLD
 end
";
            let results = apply_patch_with_loader(diff, |path| {
                assert_eq!(path, "test.txt");
                Ok("hello\nworld\nend\n".to_string())
            })
            .unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].0, "test.txt");
            assert_eq!(results[0].1, "hello\nWORLD\nend\n");
        }

        #[test]
        fn apply_hunks_preserves_no_final_newline() {
            let original = "line1\nline2";
            let hunks = vec![Hunk {
                old_start: 2,
                old_count: 1,
                new_start: 2,
                new_count: 1,
                lines: vec![
                    PatchLine::Remove("line2".into()),
                    PatchLine::Add("LINE2".into()),
                ],
            }];
            let result = apply_hunks(original, &hunks).unwrap();
            assert_eq!(result, "line1\nLINE2");
        }
    }
}
