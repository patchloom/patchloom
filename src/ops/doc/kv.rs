//! Line-oriented `.env` / `.ini` / `.properties` parse and comment-preserving splice.

use super::FileFormat;
use serde_json::{Map, Value};

pub fn parse_kv(content: &str, format: FileFormat) -> anyhow::Result<Value> {
    match format {
        FileFormat::Env => Ok(Value::Object(parse_env(content))),
        FileFormat::Ini => parse_ini(content),
        FileFormat::Properties => Ok(Value::Object(parse_properties(content))),
        _ => Err(crate::exit::InvalidInputError {
            msg: "internal: parse_kv called for a non-kv format".into(),
        }
        .into()),
    }
}

pub fn serialize_kv(value: &Value, format: FileFormat) -> anyhow::Result<String> {
    match format {
        FileFormat::Env => serialize_env(value),
        FileFormat::Ini => serialize_ini(value),
        FileFormat::Properties => serialize_properties(value),
        _ => Err(crate::exit::InvalidInputError {
            msg: "internal: serialize_kv called for a non-kv format".into(),
        }
        .into()),
    }
}

pub fn serialize_kv_preserving(
    original: &str,
    old_value: &Value,
    new_value: &Value,
    format: FileFormat,
) -> anyhow::Result<String> {
    if old_value == new_value {
        return Ok(original.to_string());
    }
    if original.trim().is_empty() {
        return serialize_kv(new_value, format);
    }
    match try_splice(original, old_value, new_value, format) {
        Some(text) => Ok(text),
        None => serialize_kv(new_value, format),
    }
}

/// True when a preserving write dropped original comment lines.
pub fn kv_comments_dropped(original: &str, new_text: &str) -> bool {
    original.lines().any(|line| {
        let t = line.trim_start();
        is_comment_line(t) && !new_text.lines().any(|n| n == line)
    })
}

fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with('#') || trimmed.starts_with(';') || trimmed.starts_with('!')
}

fn parse_env(content: &str) -> Map<String, Value> {
    let mut map = Map::new();
    for line in content.lines() {
        if let Some((key, val)) = parse_env_assignment(line) {
            map.insert(key, Value::String(val));
        }
    }
    map
}

fn parse_env_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let rest = trimmed.strip_prefix("export ").unwrap_or(trimmed).trim();
    let eq = rest.find('=')?;
    let key = rest[..eq].trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    let raw = &rest[eq + 1..];
    Some((key.to_string(), env_value_and_comment(raw).0))
}

/// Value plus a trailing `#` comment suffix (including its leading whitespace).
///
/// Docker Compose env-file rules used here: a `#` comment after whitespace
/// on an unquoted value, or after the closing quote. A `#` glued to the
/// value, or inside quotes, stays in the value. `\"`, `\\`, and `\'` are
/// escapes so a quote can appear inside a quoted value. Other backslash
/// sequences stay literal.
fn env_value_and_comment(raw: &str) -> (String, &str) {
    if raw.is_empty() {
        return (String::new(), "");
    }
    let lead = raw.len() - raw.trim_start().len();
    let body = &raw[lead..];
    if body.is_empty() {
        return (String::new(), "");
    }
    // `KEY= # note` is an empty value. `KEY=#note` keeps the `#`.
    if body.starts_with('#') {
        if lead == 0 {
            return (body.trim_end().to_string(), "");
        }
        return (String::new(), raw);
    }
    if let Some(quote @ ('"' | '\'')) = body.chars().next()
        && let Some((value, after)) = scan_env_quoted(body, quote)
    {
        let trimmed = after.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return (value, after);
        }
    }
    let (value, comment) = split_unquoted_env_comment(body);
    (value.to_string(), comment)
}

fn scan_env_quoted(raw: &str, quote: char) -> Option<(String, &str)> {
    let mut chars = raw.char_indices();
    if chars.next().map(|(_, c)| c) != Some(quote) {
        return None;
    }
    let mut out = String::new();
    while let Some((idx, c)) = chars.next() {
        if c == '\\' {
            let (_, next) = chars.next()?;
            if quote == '"' && matches!(next, '\\' | '"' | '\'') {
                out.push(next);
            } else if quote == '\'' && next == '\'' {
                out.push('\'');
            } else {
                out.push('\\');
                out.push(next);
            }
            continue;
        }
        if c == quote {
            return Some((out, &raw[idx + c.len_utf8()..]));
        }
        out.push(c);
    }
    None
}

fn split_unquoted_env_comment(raw: &str) -> (&str, &str) {
    let mut comment_at = None;
    for (idx, c) in raw.char_indices() {
        if c == '#' && idx > 0 {
            let prev = raw[..idx].chars().next_back();
            if prev.is_some_and(|p| p.is_whitespace()) {
                comment_at = Some(idx);
                break;
            }
        }
    }
    let Some(hash_at) = comment_at else {
        return (raw.trim_end(), "");
    };
    let mut start = hash_at;
    while start > 0 {
        let Some(prev) = raw[..start].chars().next_back() else {
            break;
        };
        if prev.is_whitespace() {
            start -= prev.len_utf8();
        } else {
            break;
        }
    }
    (&raw[..start], &raw[start..])
}

fn env_inline_comment_suffix(line: &str) -> String {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("export ").unwrap_or(rest);
    let Some(eq) = rest.find('=') else {
        return String::new();
    };
    env_value_and_comment(&rest[eq + 1..]).1.to_string()
}

fn parse_properties(content: &str) -> Map<String, Value> {
    let mut map = Map::new();
    for line in content.lines() {
        if let Some((key, val)) = parse_prop_assignment(line) {
            map.insert(key, Value::String(val));
        }
    }
    map
}

fn parse_prop_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }
    let sep = trimmed.find(['=', ':'])?;
    let key = trimmed[..sep].trim();
    if key.is_empty() {
        return None;
    }
    Some((
        key.to_string(),
        decode_properties_value(trimmed[sep + 1..].trim()),
    ))
}

fn parse_ini(content: &str) -> anyhow::Result<Value> {
    let mut root = Map::new();
    let mut section: Option<String> = None;
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') {
            let Some(name) = parse_ini_section(trimmed) else {
                return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                    msg: format!("invalid ini section header on line {}: {trimmed}", idx + 1),
                }));
            };
            section = Some(name);
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq].trim();
        if key.is_empty() {
            continue;
        }
        let val = Value::String(trimmed[eq + 1..].trim().to_string());
        match &section {
            Some(sec) => {
                let obj = root
                    .entry(sec.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(m) = obj {
                    m.insert(key.to_string(), val);
                }
            }
            None => {
                root.insert(key.to_string(), val);
            }
        }
    }
    Ok(Value::Object(root))
}

/// `[name]`, optionally followed by an inline `;` or `#` comment.
/// A `]` inside that comment is still a comment.
fn parse_ini_section(trimmed: &str) -> Option<String> {
    if !trimmed.starts_with('[') {
        return None;
    }
    let end = trimmed.find(']')?;
    let inner = &trimmed[1..end];
    if inner.is_empty() || inner.contains('[') {
        return None;
    }
    let rest = trimmed[end + 1..].trim_start();
    if rest.is_empty() || rest.starts_with(';') || rest.starts_with('#') {
        Some(inner.to_string())
    } else {
        None
    }
}

/// Java `Properties` keeps quotes. Escape `\`, newlines, tabs, and leading
/// whitespace. Do not wrap the value in quotes.
fn encode_properties_value(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while matches!(chars.peek(), Some(c) if *c == ' ' || *c == '\t') {
        let c = chars.next().unwrap();
        if c == '\t' {
            out.push_str("\\t");
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    for c in chars {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn decode_properties_value(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if hex.len() == 4
                    && let Ok(cp) = u32::from_str_radix(&hex, 16)
                    && let Some(ch) = char::from_u32(cp)
                {
                    out.push(ch);
                } else {
                    out.push('u');
                    out.push_str(&hex);
                }
            }
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn quote_if_needed(s: &str) -> String {
    if s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || c == '#' || c == ';' || c == '=')
    {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn value_as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

fn serialize_env(value: &Value) -> anyhow::Result<String> {
    let obj = value
        .as_object()
        .ok_or_else(|| crate::exit::InvalidInputError {
            msg: ".env documents must be a flat object".into(),
        })?;
    let mut out = String::new();
    for (k, v) in obj {
        let s = value_as_string(v).ok_or_else(|| crate::exit::InvalidInputError {
            msg: format!(".env key {k} must be a scalar"),
        })?;
        out.push_str(k);
        out.push('=');
        out.push_str(&quote_if_needed(&s));
        out.push('\n');
    }
    Ok(out)
}

fn serialize_properties(value: &Value) -> anyhow::Result<String> {
    let obj = value
        .as_object()
        .ok_or_else(|| crate::exit::InvalidInputError {
            msg: ".properties documents must be a flat object".into(),
        })?;
    let mut out = String::new();
    for (k, v) in obj {
        let s = value_as_string(v).ok_or_else(|| crate::exit::InvalidInputError {
            msg: format!(".properties key {k} must be a scalar"),
        })?;
        out.push_str(k);
        out.push('=');
        out.push_str(&encode_properties_value(&s));
        out.push('\n');
    }
    Ok(out)
}

fn serialize_ini(value: &Value) -> anyhow::Result<String> {
    let obj = value
        .as_object()
        .ok_or_else(|| crate::exit::InvalidInputError {
            msg: ".ini documents must be an object".into(),
        })?;
    let mut out = String::new();
    for (k, v) in obj {
        match v {
            Value::Object(inner) => {
                out.push('[');
                out.push_str(k);
                out.push_str("]\n");
                for (ik, iv) in inner {
                    let s = value_as_string(iv).ok_or_else(|| crate::exit::InvalidInputError {
                        msg: format!(".ini key {k}.{ik} must be a scalar"),
                    })?;
                    out.push_str(ik);
                    out.push('=');
                    out.push_str(&s);
                    out.push('\n');
                }
            }
            _ => {
                let s = value_as_string(v).ok_or_else(|| crate::exit::InvalidInputError {
                    msg: format!(".ini key {k} must be a scalar or section object"),
                })?;
                out.push_str(k);
                out.push('=');
                out.push_str(&s);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn try_splice(
    original: &str,
    old_value: &Value,
    new_value: &Value,
    format: FileFormat,
) -> Option<String> {
    let old_obj = old_value.as_object()?;
    let new_obj = new_value.as_object()?;
    let eol = crate::write::detect_eol(original);
    let mut lines: Vec<String> = crate::ops::file::text_lines(original)
        .map(str::to_string)
        .collect();
    match format {
        FileFormat::Env => splice_flat(&mut lines, old_obj, new_obj, KvStyle::Env)?,
        FileFormat::Properties => splice_flat(&mut lines, old_obj, new_obj, KvStyle::Properties)?,
        FileFormat::Ini => splice_ini(&mut lines, old_obj, new_obj)?,
        _ => return None,
    }
    let mut out = lines.join(eol);
    if (original.ends_with('\n') || original.ends_with('\r')) && !out.ends_with(eol) {
        out.push_str(eol);
    }
    Some(out)
}

#[derive(Clone, Copy)]
enum KvStyle {
    Env,
    Properties,
}

fn splice_flat(
    lines: &mut Vec<String>,
    old_obj: &Map<String, Value>,
    new_obj: &Map<String, Value>,
    style: KvStyle,
) -> Option<()> {
    for (key, new_v) in new_obj {
        let new_s = value_as_string(new_v)?;
        if old_obj.get(key) == Some(new_v) {
            continue;
        }
        if let Some(idx) = find_last_flat_line(lines, key, style) {
            lines[idx] = replace_flat_value(&lines[idx], &new_s, style);
        } else {
            lines.push(format!("{key}={}", encode_flat_value(style, &new_s)));
        }
    }
    let mut remove = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let parsed = match style {
            KvStyle::Env => parse_env_assignment(line),
            KvStyle::Properties => parse_prop_assignment(line),
        };
        if let Some((k, _)) = parsed
            && !new_obj.contains_key(&k)
        {
            remove.push(i);
        }
    }
    for i in remove.into_iter().rev() {
        lines.remove(i);
    }
    Some(())
}

fn find_last_flat_line(lines: &[String], key: &str, style: KvStyle) -> Option<usize> {
    let mut hit = None;
    for (i, line) in lines.iter().enumerate() {
        let parsed = match style {
            KvStyle::Env => parse_env_assignment(line),
            KvStyle::Properties => parse_prop_assignment(line),
        };
        if parsed.is_some_and(|(k, _)| k == key) {
            hit = Some(i);
        }
    }
    hit
}

fn replace_flat_value(line: &str, new_s: &str, style: KvStyle) -> String {
    let trimmed_start = line.len() - line.trim_start().len();
    let prefix = &line[..trimmed_start];
    let rest = line.trim_start();
    let (head, suffix) = match style {
        KvStyle::Env => {
            let work = rest.strip_prefix("export ").unwrap_or(rest);
            let export = if rest.starts_with("export ") {
                "export "
            } else {
                ""
            };
            let eq = work.find('=').unwrap_or(work.len());
            (
                format!("{prefix}{export}{}=", work[..eq].trim_end()),
                env_inline_comment_suffix(line),
            )
        }
        KvStyle::Properties => {
            let sep_at = rest.find(['=', ':']).unwrap_or(rest.len());
            let sep = rest.as_bytes().get(sep_at).copied().unwrap_or(b'=') as char;
            (
                format!("{prefix}{}{sep}", rest[..sep_at].trim_end()),
                String::new(),
            )
        }
    };
    format!("{head}{}{suffix}", encode_flat_value(style, new_s))
}

fn encode_flat_value(style: KvStyle, s: &str) -> String {
    match style {
        KvStyle::Env => quote_if_needed(s),
        KvStyle::Properties => encode_properties_value(s),
    }
}

fn splice_ini(
    lines: &mut Vec<String>,
    old_obj: &Map<String, Value>,
    new_obj: &Map<String, Value>,
) -> Option<()> {
    for (key, new_v) in new_obj {
        match new_v {
            Value::Object(inner) => {
                let old_inner = old_obj.get(key).and_then(Value::as_object);
                ensure_ini_section(lines, key);
                for (ik, iv) in inner {
                    let new_s = value_as_string(iv)?;
                    if old_inner.and_then(|m| m.get(ik)) == Some(iv) {
                        continue;
                    }
                    if let Some(idx) = find_ini_key_line(lines, key, ik) {
                        lines[idx] = replace_ini_value(&lines[idx], &new_s);
                    } else if let Some(end) = ini_section_end(lines, key) {
                        lines.insert(end, format!("{ik}={new_s}"));
                    }
                }
                if let Some(old_inner) = old_inner {
                    let mut remove = Vec::new();
                    for (i, line) in lines.iter().enumerate() {
                        if ini_line_section(lines, i).as_deref() == Some(key.as_str())
                            && let Some((k, _)) = parse_ini_assignment(line)
                            && !inner.contains_key(&k)
                            && old_inner.contains_key(&k)
                        {
                            remove.push(i);
                        }
                    }
                    for i in remove.into_iter().rev() {
                        lines.remove(i);
                    }
                }
            }
            _ => {
                let new_s = value_as_string(new_v)?;
                if old_obj.get(key) == Some(new_v) {
                    continue;
                }
                if let Some(idx) = find_ini_global_key(lines, key) {
                    lines[idx] = replace_ini_value(&lines[idx], &new_s);
                } else {
                    lines.push(format!("{key}={new_s}"));
                }
            }
        }
    }
    Some(())
}

fn parse_ini_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with(';')
        || parse_ini_section(trimmed).is_some()
    {
        return None;
    }
    let eq = trimmed.find('=')?;
    let key = trimmed[..eq].trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), trimmed[eq + 1..].trim().to_string()))
}

fn ini_line_section(lines: &[String], idx: usize) -> Option<String> {
    let mut cur = None;
    for line in lines.iter().take(idx + 1) {
        if let Some(name) = parse_ini_section(line.trim()) {
            cur = Some(name);
        }
    }
    cur
}

fn find_ini_key_line(lines: &[String], section: &str, key: &str) -> Option<usize> {
    let mut in_section = false;
    let mut hit = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if let Some(name) = parse_ini_section(t) {
            in_section = name == section;
            continue;
        }
        if in_section && parse_ini_assignment(line).is_some_and(|(k, _)| k == key) {
            hit = Some(i);
        }
    }
    hit
}

fn find_ini_global_key(lines: &[String], key: &str) -> Option<usize> {
    let mut in_section = false;
    let mut hit = None;
    for (i, line) in lines.iter().enumerate() {
        if parse_ini_section(line.trim()).is_some() {
            in_section = true;
            continue;
        }
        if !in_section && parse_ini_assignment(line).is_some_and(|(k, _)| k == key) {
            hit = Some(i);
        }
    }
    hit
}

fn ensure_ini_section(lines: &mut Vec<String>, section: &str) {
    if lines
        .iter()
        .any(|l| parse_ini_section(l.trim()).as_deref() == Some(section))
    {
        return;
    }
    lines.push(format!("[{section}]"));
}

fn ini_section_end(lines: &[String], section: &str) -> Option<usize> {
    let mut in_section = false;
    let mut end = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(name) = parse_ini_section(line.trim()) {
            if in_section {
                return Some(i);
            }
            in_section = name == section;
            if in_section {
                end = Some(i + 1);
            }
            continue;
        }
        if in_section {
            end = Some(i + 1);
        }
    }
    end
}

fn replace_ini_value(line: &str, new_s: &str) -> String {
    let trimmed_start = line.len() - line.trim_start().len();
    let prefix = &line[..trimmed_start];
    let rest = line.trim_start();
    let eq = rest.find('=').unwrap_or(rest.len());
    format!("{prefix}{}={}", rest[..eq].trim_end(), new_s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn properties_space_is_not_quoted_and_roundtrips() {
        let orig = "# app config\napp.name=demo\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        let mut new = old.clone();
        new["app.title"] = json!("Hello World");
        let text = serialize_kv_preserving(orig, &old, &new, FileFormat::Properties).unwrap();
        assert!(
            text.contains("app.title=Hello World\n"),
            "must not wrap the value in quotes: {text:?}"
        );
        assert!(!text.contains("app.title=\""));
        assert!(text.contains("# app config"));
        let got = parse_kv(&text, FileFormat::Properties).unwrap();
        assert_eq!(got["app.title"], json!("Hello World"));
    }

    #[test]
    fn properties_literal_quotes_stay_in_the_value() {
        let got = parse_kv("raw.q=\"quoted\"\n", FileFormat::Properties).unwrap();
        assert_eq!(got["raw.q"], json!("\"quoted\""));
    }

    #[test]
    fn ini_space_is_not_quoted_and_roundtrips() {
        let orig = "; c\n[s]\nk=1\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["s"]["k"] = json!("x y");
        let text = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(text.contains("k=x y\n"), "{text:?}");
        assert!(!text.contains("k=\""));
        assert!(text.contains("; c"));
        let got = parse_kv(&text, FileFormat::Ini).unwrap();
        assert_eq!(got["s"]["k"], json!("x y"));
    }

    #[test]
    fn ini_literal_quotes_stay_in_the_value() {
        let got = parse_kv("[s]\nk=\"x y\"\n", FileFormat::Ini).unwrap();
        assert_eq!(got["s"]["k"], json!("\"x y\""));
    }

    #[test]
    fn env_space_still_quotes_and_strips_on_read() {
        let text = serialize_kv(&json!({"A": "x y"}), FileFormat::Env).unwrap();
        assert!(text.contains("A=\"x y\""), "{text:?}");
        let got = parse_kv(&text, FileFormat::Env).unwrap();
        assert_eq!(got["A"], json!("x y"));
    }

    #[test]
    fn env_parse_export_and_comment() {
        let val = parse_kv("# keep\nexport A=1\nB=two\n", FileFormat::Env).unwrap();
        assert_eq!(val["A"], json!("1"));
        assert_eq!(val["B"], json!("two"));
    }

    #[test]
    fn env_inline_comment_is_not_part_of_the_value() {
        let val = parse_kv(
            "A=1 # local\nB=\"two\" # note\nC=\"hash#inside\"\nD='sq # keep'\nE=foo#bar\nF=\"say \\\"hi\\\"\"\nG='Let\\'s go!'\nH=\"a\\nb\"\nexport I=9 # shipped\nJ = 10 # spaced\nK='two' # sq\nL= # blank\nM=#keep\n",
            FileFormat::Env,
        )
        .unwrap();
        assert_eq!(val["A"], json!("1"));
        assert_eq!(val["B"], json!("two"));
        assert_eq!(val["C"], json!("hash#inside"));
        assert_eq!(val["D"], json!("sq # keep"));
        assert_eq!(val["E"], json!("foo#bar"));
        assert_eq!(val["F"], json!("say \"hi\""));
        assert_eq!(val["G"], json!("Let's go!"));
        assert_eq!(val["H"], json!("a\\nb"));
        assert_eq!(val["I"], json!("9"));
        assert_eq!(val["J"], json!("10"));
        assert_eq!(val["K"], json!("two"));
        assert_eq!(val["L"], json!(""));
        assert_eq!(val["M"], json!("#keep"));
    }

    #[test]
    fn env_set_keeps_inline_comment() {
        let orig = "export A=1 # local\nB=\"two\" # note\nL= # blank\n";
        let old = parse_kv(orig, FileFormat::Env).unwrap();
        assert_eq!(old["A"], json!("1"));
        assert_eq!(old["L"], json!(""));
        let mut new = old.clone();
        new["A"] = json!("2");
        new["B"] = json!("three");
        new["L"] = json!("x");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Env).unwrap();
        assert!(out.contains("export A=2 # local"), "{out:?}");
        assert!(out.contains("B=three # note"), "{out:?}");
        assert!(out.contains("L=x # blank"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Env).unwrap();
        assert_eq!(got["A"], json!("2"));
        assert_eq!(got["B"], json!("three"));
    }

    #[test]
    fn env_set_keeps_comment() {
        let orig = "# keep\nA=1\n";
        let old = parse_kv(orig, FileFormat::Env).unwrap();
        let new = json!({"A": "2"});
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Env).unwrap();
        assert!(out.contains("# keep"));
        assert!(out.contains("A=2"));
    }

    #[test]
    fn ini_unclosed_section_is_parse_error() {
        let err = parse_kv("[db]\nport=1\n[broken\nport=2\n", FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_parse_error(&err), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("line 3"), "{msg}");
        assert!(msg.contains("[broken"), "{msg}");
    }

    #[test]
    fn ini_section_inline_comment_stays_in_section() {
        let got = parse_kv("[db] ; primary\nport=1\n", FileFormat::Ini).unwrap();
        assert_eq!(got["db"]["port"], json!("1"));
        assert!(got.get("port").is_none());
    }

    #[test]
    fn ini_section_comment_may_contain_bracket() {
        for src in [
            "[db] ; see [backup]\nport=1\n",
            "[db] # range [1, 2]\nport=1\n",
        ] {
            let got = parse_kv(src, FileFormat::Ini).unwrap();
            assert_eq!(got["db"]["port"], json!("1"), "{src}");
            assert!(got.get("port").is_none(), "{src}");
        }
        let orig = "[db] ; see [backup]\nport=1\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["db"]["port"] = json!("2");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("[db] ; see [backup]"), "{out}");
        assert!(out.contains("port=2"), "{out}");
    }

    #[test]
    fn ini_junk_after_header_is_parse_error() {
        for src in ["[db]]\nport=1\n", "[db] trailing\nport=1\n"] {
            let err = parse_kv(src, FileFormat::Ini).unwrap_err();
            assert!(crate::exit::is_parse_error(&err), "{src}: {err}");
        }
    }

    #[test]
    fn ini_section_get_and_set() {
        let orig = "# c\n[server]\nport=80\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        assert_eq!(old["server"]["port"], json!("80"));
        let mut new = old.clone();
        new["server"]["port"] = json!("443");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("# c"));
        assert!(out.contains("[server]"));
        assert!(out.contains("port=443"));
    }

    #[test]
    fn properties_colon_and_equals() {
        let orig = "! c\na:1\nb=2\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        assert_eq!(old["a"], json!("1"));
        assert_eq!(old["b"], json!("2"));
        let mut new = old.clone();
        new["a"] = json!("9");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Properties).unwrap();
        assert!(out.contains("! c"));
        assert!(out.contains("a:9") || out.contains("a=9"));
    }

    #[test]
    fn properties_dotted_selector_is_one_key() {
        assert_eq!(
            crate::ops::doc::rewrite_selector_for_format("server.port", FileFormat::Properties)
                .as_ref(),
            "\"server.port\""
        );
        assert_eq!(
            crate::ops::doc::rewrite_selector_for_format("name", FileFormat::Properties).as_ref(),
            "name"
        );
        assert_eq!(
            crate::ops::doc::rewrite_selector_for_format("server.port", FileFormat::Json).as_ref(),
            "server.port"
        );
        assert_eq!(
            crate::ops::doc::rewrite_selector_for_format("main.name", FileFormat::Ini).as_ref(),
            "main.name"
        );
    }
}
