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

    const MAX_MERGE_DEPTH: usize = 128;

    pub(crate) fn deep_merge(base: &mut serde_json::Value, other: &serde_json::Value) {
        deep_merge_inner(base, other, 0);
    }

    fn deep_merge_inner(base: &mut serde_json::Value, other: &serde_json::Value, depth: usize) {
        if depth >= MAX_MERGE_DEPTH {
            *base = other.clone();
            return;
        }
        if base.is_object() && other.is_object() {
            let other_map = other.as_object().unwrap();
            let base_map = base.as_object_mut().unwrap();
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
