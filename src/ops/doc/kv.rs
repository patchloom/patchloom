// size-waiver: accepted single-domain bulk (policy #1408). Env, ini, and properties share one reader and one splice.
//! Line-oriented `.env` / `.ini` / `.properties` parse and comment-preserving splice.

use super::FileFormat;
use serde_json::{Map, Value};

pub fn parse_kv(content: &str, format: FileFormat) -> anyhow::Result<Value> {
    match format {
        FileFormat::Env => Ok(Value::Object(parse_env(content))),
        FileFormat::Ini => parse_ini(content),
        FileFormat::Properties => Ok(Value::Object(parse_properties(content)?)),
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
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if let Some((key, raw)) = split_env_key_raw(lines[i]) {
            let (val, used) = read_env_value(&lines[i..], raw);
            map.insert(key, Value::String(val));
            i += used.max(1);
        } else {
            i += 1;
        }
    }
    map
}

/// Key and the raw text after `=`. Comments and blank lines are [`None`].
fn split_env_key_raw(line: &str) -> Option<(String, &str)> {
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
    Some((key.to_string(), &rest[eq + 1..]))
}

/// A quoted value may continue on later physical lines. `used` is how many
/// lines belong to this assignment. An unclosed quote stays on the first line.
fn read_env_value(lines: &[&str], raw0: &str) -> (String, usize) {
    let single = env_value_and_comment(raw0).0;
    let body = raw0.trim_start();
    let Some(quote @ ('"' | '\'')) = body.chars().next() else {
        return (single, 1);
    };
    if quoted_env_closed(body, quote) {
        return (single, 1);
    }
    let mut joined = body.to_string();
    for (offset, extra) in lines.iter().enumerate().skip(1) {
        joined.push('\n');
        joined.push_str(extra);
        if let Some((value, after)) = scan_env_quoted(&joined, quote) {
            let rest = after.trim_start();
            if rest.is_empty() || rest.starts_with('#') {
                return (value, offset + 1);
            }
            return (single, 1);
        }
    }
    (single, 1)
}

fn quoted_env_closed(body: &str, quote: char) -> bool {
    scan_env_quoted(body, quote).is_some_and(|(_, after)| {
        let rest = after.trim_start();
        rest.is_empty() || rest.starts_with('#')
    })
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
    let suffix = env_value_and_comment(&rest[eq + 1..]).1;
    // `"two"#note` is a comment on read. An unquoted replacement would glue
    // the hash onto the value (`three#note`). One space keeps it a comment.
    if suffix.starts_with('#') {
        format!(" {suffix}")
    } else {
        suffix.to_string()
    }
}

fn parse_properties(content: &str) -> anyhow::Result<Map<String, Value>> {
    let mut map = Map::new();
    let lines: Vec<&str> = content.lines().collect();
    for line in logical_properties_lines(&lines) {
        match parse_prop_assignment(&line.text) {
            Ok(Some((key, val))) => {
                map.insert(key, Value::String(val));
            }
            Ok(None) => {}
            Err(detail) => {
                return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                    msg: format!(
                        "malformed \\u escape in .properties on line {}: {detail}",
                        line.line_no
                    ),
                }));
            }
        }
    }
    Ok(map)
}

fn properties_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#') || trimmed.starts_with('!')
}

fn properties_line_continues(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut n = 0usize;
    while n < bytes.len() && bytes[bytes.len() - 1 - n] == b'\\' {
        n += 1;
    }
    n % 2 == 1
}

struct LogicalPropLine {
    start: usize,
    end: usize,
    line_no: usize,
    text: String,
}

/// Java logical lines: an odd trailing `\` joins the next natural line and
/// drops that backslash. Leading whitespace on the continued line is not
/// part of the value. A `#` or `!` line is skipped whole, so that backslash
/// does not continue it. With no next line, the continuation backslash is
/// dropped. Read and preserving write both use this so a wrapped value is
/// one key.
fn logical_properties_lines(lines: &[impl AsRef<str>]) -> Vec<LogicalPropLine> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        let mut text = lines[i].as_ref().to_string();
        if !properties_comment_line(&text) {
            while properties_line_continues(&text) && i + 1 < lines.len() {
                text.pop();
                i += 1;
                text.push_str(lines[i].as_ref().trim_start());
            }
            if properties_line_continues(&text) {
                text.pop();
            }
        }
        i += 1;
        out.push(LogicalPropLine {
            start,
            end: i,
            line_no: start + 1,
            text,
        });
    }
    out
}

fn parse_prop_assignment(line: &str) -> Result<Option<(String, String)>, &'static str> {
    let Some(split) = split_properties_entry(line.trim_start_matches(is_prop_whitespace)) else {
        return Ok(None);
    };
    let key = decode_properties_escapes(split.key_raw)?;
    if key.is_empty() {
        return Ok(None);
    }
    let val = decode_properties_escapes(split.value_raw)?;
    Ok(Some((key, val)))
}

/// Space, tab, and form feed end a Java properties key. Line breaks do not.
fn is_prop_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{000c}')
}

struct PropSplit<'a> {
    key_raw: &'a str,
    value_raw: &'a str,
    /// `=` or `:` when the line uses one, including after the key's whitespace.
    delim: Option<char>,
}

/// Split one logical properties line the way `Properties.load` does.
///
/// The key runs to the first unescaped `=`, `:`, space, tab, or form feed.
/// Whitespace after that is skipped, then one optional `=` or `:`. The rest
/// is the value, including trailing spaces. A line with no separator is
/// [`None`]. A blank line, a comment, or an empty key is [`None`].
fn split_properties_entry(line: &str) -> Option<PropSplit<'_>> {
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let mut escaped = false;
    let mut sep_at = None;
    let mut sep_is_delim = false;
    for (idx, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '=' || c == ':' {
            sep_at = Some(idx);
            sep_is_delim = true;
            break;
        }
        if is_prop_whitespace(c) {
            sep_at = Some(idx);
            sep_is_delim = false;
            break;
        }
    }
    let sep_at = sep_at?;
    if sep_at == 0 {
        return None;
    }
    let sep_char = line[sep_at..].chars().next()?;
    let mut rest = &line[sep_at + sep_char.len_utf8()..];
    let mut delim = if sep_is_delim { Some(sep_char) } else { None };
    rest = rest.trim_start_matches(is_prop_whitespace);
    if delim.is_none()
        && let Some(c @ ('=' | ':')) = rest.chars().next()
    {
        delim = Some(c);
        rest = &rest[c.len_utf8()..];
        rest = rest.trim_start_matches(is_prop_whitespace);
    }
    Some(PropSplit {
        key_raw: &line[..sep_at],
        value_raw: rest,
        delim,
    })
}

fn ini_separator_index(text: &str) -> Option<usize> {
    text.find(['=', ':'])
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
            if root.get(&name).is_some_and(|v| !v.is_object()) {
                return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                    msg: format!(
                        "ini section [{name}] on line {} collides with a key of the same name",
                        idx + 1
                    ),
                }));
            }
            // Record the header before any key. An empty section must
            // round-trip, or the next preserving edit treats it as deleted.
            root.entry(name.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            section = Some(name);
            continue;
        }
        let Some((key, raw_val)) = parse_ini_assignment(line) else {
            return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                msg: format!("invalid ini assignment on line {}: {trimmed}", idx + 1),
            }));
        };
        let val = Value::String(raw_val);
        match &section {
            Some(sec) => {
                let obj = root
                    .entry(sec.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(m) = obj {
                    m.insert(key, val);
                }
            }
            None => {
                root.insert(key, val);
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
    let inner = trimmed[1..end].trim();
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

/// Escape `\`, separators, newlines, and a leading `#` or `!` so a new key
/// round-trips through [`parse_prop_assignment`].
fn encode_properties_key(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{000c}' => out.push_str("\\f"),
            ' ' | '=' | ':' => {
                out.push('\\');
                out.push(c);
            }
            '#' | '!' if i == 0 => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out
}

/// Java `Properties` keeps quotes. Escape `\`, newlines, tabs, form feeds,
/// and leading whitespace. Do not wrap the value in quotes.
fn encode_properties_value(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while matches!(chars.peek(), Some(c) if *c == ' ' || *c == '\t' || *c == '\u{000c}') {
        let c = chars.next().unwrap();
        match c {
            '\t' => out.push_str("\\t"),
            '\u{000c}' => out.push_str("\\f"),
            _ => {
                out.push('\\');
                out.push(c);
            }
        }
    }
    for c in chars {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{000c}' => out.push_str("\\f"),
            other => out.push(other),
        }
    }
    out
}

fn decode_properties_escapes(s: &str) -> Result<String, &'static str> {
    if !s.contains('\\') {
        return Ok(s.to_string());
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
            Some('f') => out.push('\u{000c}'),
            Some('u') => out.push(decode_properties_unicode(&mut chars)?),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Ok(out)
}

/// Four hex digits, or a `\u` surrogate pair, as one Unicode scalar.
fn decode_properties_unicode(chars: &mut impl Iterator<Item = char>) -> Result<char, &'static str> {
    let unit = read_properties_hex4(chars)?;
    if (0xD800..=0xDBFF).contains(&unit) {
        if chars.next() != Some('\\') || chars.next() != Some('u') {
            return Err("\\u is not a Unicode character");
        }
        let low = read_properties_hex4(chars)?;
        if !(0xDC00..=0xDFFF).contains(&low) {
            return Err("\\u is not a Unicode character");
        }
        let cp = 0x10000 + (((unit - 0xD800) << 10) | (low - 0xDC00));
        return char::from_u32(cp).ok_or("\\u is not a Unicode character");
    }
    if (0xDC00..=0xDFFF).contains(&unit) {
        return Err("\\u is not a Unicode character");
    }
    char::from_u32(unit).ok_or("\\u is not a Unicode character")
}

fn read_properties_hex4(chars: &mut impl Iterator<Item = char>) -> Result<u32, &'static str> {
    let mut hex = String::with_capacity(4);
    for _ in 0..4 {
        match chars.next() {
            Some(c) if c.is_ascii_hexdigit() => hex.push(c),
            _ => return Err("\\u needs four hex digits"),
        }
    }
    u32::from_str_radix(&hex, 16).map_err(|_| "\\u needs four hex digits")
}

fn quote_if_needed(s: &str) -> String {
    let needs = s.is_empty()
        || s.chars().any(|c| {
            c.is_whitespace() || matches!(c, '#' | ';' | '=' | '"' | '\'' | '\\' | '\n' | '\r')
        });
    if !needs {
        return s.to_string();
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '\'') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
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

fn breaks_kv_line(s: &str) -> bool {
    s.chars().any(|c| c == '\n' || c == '\r')
}

/// Env readers split on the first `=`. Ini readers split on the first `=`
/// or `:`. Neither treats `\` as an escape in the key. A key or value that
/// would parse as another line is rejected.
fn env_key_reads_back(key: &str, value: &str) -> bool {
    if breaks_kv_line(key) || value.contains('\r') {
        return false;
    }
    let text = format!("{key}={}", quote_if_needed(value));
    parse_env(&text).get(key).and_then(Value::as_str) == Some(value)
}

fn ensure_env_key_reads_back(key: &str, value: &str) -> anyhow::Result<()> {
    if env_key_reads_back(key, value) {
        return Ok(());
    }
    let why = if breaks_kv_line(value) && !breaks_kv_line(key) {
        "the written value would split onto another line"
    } else {
        "the written line would be read back as a different key"
    };
    Err(crate::exit::InvalidInputError {
        msg: format!("invalid .env key {key}: {why}"),
    }
    .into())
}

fn ini_key_reads_back(key: &str, value: &str) -> bool {
    if breaks_kv_line(key) || breaks_kv_line(value) {
        return false;
    }
    // Trailing or leading spaces trim on read. A space before `;` or `#`
    // starts a comment, so that value would come back shorter.
    parse_ini_assignment(&format!("{key}={value}"))
        .is_some_and(|(k, v)| k == key && (v == value || v == value.trim()))
}

fn ensure_ini_key_reads_back(key: &str, value: &str) -> anyhow::Result<()> {
    if ini_key_reads_back(key, value) {
        return Ok(());
    }
    let why = if breaks_kv_line(value) && !breaks_kv_line(key) {
        "the written value would split onto another line"
    } else if parse_ini_assignment(&format!("{key}={value}"))
        .is_some_and(|(k, v)| k == key && v != value && v != value.trim())
    {
        "a space before `;` or `#` in the value starts a comment, so the value would read back shorter"
    } else {
        "the written line would be read back as a different key or an extra assignment"
    };
    Err(crate::exit::InvalidInputError {
        msg: format!("invalid .ini key {key}: {why}"),
    }
    .into())
}

fn ini_section_reads_back(name: &str) -> bool {
    if breaks_kv_line(name) {
        return false;
    }
    parse_ini_section(&format!("[{name}]")).as_deref() == Some(name)
}

fn ensure_ini_section_reads_back(name: &str) -> anyhow::Result<()> {
    if ini_section_reads_back(name) {
        return Ok(());
    }
    Err(crate::exit::InvalidInputError {
        msg: format!(
            "invalid .ini section {name}: the written header would be read back as a different section"
        ),
    }
    .into())
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
        ensure_env_key_reads_back(k, &s)?;
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
        out.push_str(&encode_properties_key(k));
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
    // Root keys first. A scalar after a header is read back as a key
    // of that section.
    for (k, v) in obj {
        if v.is_object() {
            continue;
        }
        let s = value_as_string(v).ok_or_else(|| crate::exit::InvalidInputError {
            msg: format!(".ini key {k} must be a scalar or section object"),
        })?;
        ensure_ini_key_reads_back(k, &s)?;
        out.push_str(k);
        out.push('=');
        out.push_str(&s);
        out.push('\n');
    }
    for (k, v) in obj {
        let Value::Object(inner) = v else {
            continue;
        };
        ensure_ini_section_reads_back(k)?;
        out.push('[');
        out.push_str(k);
        out.push_str("]\n");
        for (ik, iv) in inner {
            let s = value_as_string(iv).ok_or_else(|| crate::exit::InvalidInputError {
                msg: format!(".ini key {k}.{ik} must be a scalar"),
            })?;
            ensure_ini_key_reads_back(ik, &s)?;
            out.push_str(ik);
            out.push('=');
            out.push_str(&s);
            out.push('\n');
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
        FileFormat::Env => splice_flat(&mut lines, old_obj, new_obj)?,
        FileFormat::Properties => splice_properties(&mut lines, old_obj, new_obj)?,
        FileFormat::Ini => splice_ini(&mut lines, old_obj, new_obj)?,
        _ => return None,
    }
    let mut out = lines.join(eol);
    if (original.ends_with('\n') || original.ends_with('\r')) && !out.ends_with(eol) {
        out.push_str(eol);
    }
    Some(out)
}

fn splice_properties(
    lines: &mut Vec<String>,
    old_obj: &Map<String, Value>,
    new_obj: &Map<String, Value>,
) -> Option<()> {
    let spans = logical_properties_lines(lines);
    let mut range_edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut appends = Vec::new();
    for (key, new_v) in new_obj {
        let new_s = value_as_string(new_v)?;
        if old_obj.get(key) == Some(new_v) {
            continue;
        }
        let mut found = None;
        for span in spans.iter().rev() {
            match parse_prop_assignment(&span.text) {
                Ok(Some((k, _))) if k == *key => {
                    found = Some((span.start, span.end, span.text.clone()));
                    break;
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        if let Some((start, end, text)) = found {
            let rewritten = replace_properties_line(&text, &new_s)?;
            range_edits.push((start, end, vec![rewritten]));
        } else {
            appends.push(format!(
                "{}={}",
                encode_properties_key(key),
                encode_properties_value(&new_s)
            ));
        }
    }
    for span in &spans {
        if range_edits.iter().any(|(start, _, _)| *start == span.start) {
            continue;
        }
        match parse_prop_assignment(&span.text) {
            Ok(Some((k, _))) if !new_obj.contains_key(&k) => {
                range_edits.push((span.start, span.end, Vec::new()));
            }
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    range_edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    for (start, end, new_lines) in range_edits {
        lines.splice(start..end, new_lines);
    }
    lines.extend(appends);
    Some(())
}

/// Rewrite one logical properties line. Leading space, tab, and form feed
/// stay. The separator character stays when the line used `=` or `:`.
fn replace_properties_line(line: &str, new_s: &str) -> Option<String> {
    let lead = line.len() - line.trim_start_matches(is_prop_whitespace).len();
    let prefix = &line[..lead];
    let split = split_properties_entry(&line[lead..])?;
    let sep = split.delim.unwrap_or('=');
    Some(format!(
        "{prefix}{}{sep}{}",
        split.key_raw,
        encode_properties_value(new_s)
    ))
}

struct EnvSpan {
    start: usize,
    end: usize,
    assignment: Option<(String, String)>,
}

fn logical_env_spans(lines: &[String]) -> Vec<EnvSpan> {
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < refs.len() {
        if let Some((key, raw)) = split_env_key_raw(refs[i]) {
            let (val, used) = read_env_value(&refs[i..], raw);
            let used = used.max(1);
            out.push(EnvSpan {
                start: i,
                end: i + used,
                assignment: Some((key, val)),
            });
            i += used;
        } else {
            out.push(EnvSpan {
                start: i,
                end: i + 1,
                assignment: None,
            });
            i += 1;
        }
    }
    out
}

fn splice_flat(
    lines: &mut Vec<String>,
    old_obj: &Map<String, Value>,
    new_obj: &Map<String, Value>,
) -> Option<()> {
    let spans = logical_env_spans(lines);
    let mut range_edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut appends = Vec::new();
    for (key, new_v) in new_obj {
        let new_s = value_as_string(new_v)?;
        if old_obj.get(key) == Some(new_v) {
            continue;
        }
        // None falls through to serialize_env, which rejects the key.
        if !env_key_reads_back(key, &new_s) {
            return None;
        }
        if let Some(span) = spans
            .iter()
            .rev()
            .find(|span| span.assignment.as_ref().is_some_and(|(k, _)| k == key))
        {
            let rewritten = if span.end - span.start == 1 && !new_s.contains('\n') {
                replace_env_value(&lines[span.start], &new_s)
            } else {
                format!("{key}={}", quote_if_needed(&new_s))
            };
            range_edits.push((span.start, span.end, vec![rewritten]));
        } else {
            appends.push(format!("{key}={}", quote_if_needed(&new_s)));
        }
    }
    for span in &spans {
        if range_edits.iter().any(|(start, _, _)| *start == span.start) {
            continue;
        }
        if let Some((k, _)) = &span.assignment
            && !new_obj.contains_key(k)
        {
            range_edits.push((span.start, span.end, Vec::new()));
        }
    }
    range_edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    for (start, end, new_lines) in range_edits {
        lines.splice(start..end, new_lines);
    }
    lines.extend(appends);
    Some(())
}

fn replace_env_value(line: &str, new_s: &str) -> String {
    let trimmed_start = line.len() - line.trim_start().len();
    let prefix = &line[..trimmed_start];
    let rest = line.trim_start();
    let work = rest.strip_prefix("export ").unwrap_or(rest);
    let export = if rest.starts_with("export ") {
        "export "
    } else {
        ""
    };
    let eq = work.find('=').unwrap_or(work.len());
    format!(
        "{prefix}{export}{}={}{}",
        work[..eq].trim_end(),
        quote_if_needed(new_s),
        env_inline_comment_suffix(line)
    )
}

/// Drop root keys and sections the new object no longer has.
///
/// The update loop only rewrites keys that remain. Without this pass,
/// `doc delete` of a root key or a whole section reports `removed: 1`
/// and leaves the old assignment on disk. A section that became a
/// scalar, or a root key that became a section, would also leave the
/// old line beside the new one.
fn drop_stale_ini_lines(lines: &mut Vec<String>, new_obj: &Map<String, Value>) {
    let mut remove = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(name) = parse_ini_section(lines[i].trim()) {
            let keep_section = matches!(new_obj.get(&name), Some(Value::Object(_)));
            if !keep_section {
                remove.push(i);
                i += 1;
                while i < lines.len() && parse_ini_section(lines[i].trim()).is_none() {
                    if parse_ini_assignment(&lines[i]).is_some() {
                        remove.push(i);
                    }
                    i += 1;
                }
                continue;
            }
        }
        i += 1;
    }
    for (i, line) in lines.iter().enumerate() {
        if remove.contains(&i) || ini_line_section(lines, i).is_some() {
            continue;
        }
        if let Some((key, _)) = parse_ini_assignment(line) {
            match new_obj.get(&key) {
                None | Some(Value::Object(_)) => remove.push(i),
                Some(_) => {}
            }
        }
    }
    remove.sort_unstable();
    remove.dedup();
    for i in remove.into_iter().rev() {
        lines.remove(i);
    }
}

/// A new root assignment must sit before the first section header.
///
/// Appending at EOF puts the line inside the last section, so the next
/// read treats it as that section's key.
fn insert_ini_root_line(lines: &mut Vec<String>, line: String) {
    if let Some(idx) = lines
        .iter()
        .position(|existing| parse_ini_section(existing.trim()).is_some())
    {
        lines.insert(idx, line);
    } else {
        lines.push(line);
    }
}

/// Remove keys the section object no longer has, in one forward pass.
///
/// `ini_line_section` restarts from line 0, so calling it on every line
/// of every section makes a one-key update scan the file once per section.
fn remove_omitted_ini_section_keys(
    lines: &mut Vec<String>,
    section: &str,
    inner: &Map<String, Value>,
    old_inner: &Map<String, Value>,
) {
    let mut remove = Vec::new();
    let mut current: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(name) = parse_ini_section(line.trim()) {
            current = Some(name);
            continue;
        }
        if current.as_deref() != Some(section) {
            continue;
        }
        if let Some((key, _)) = parse_ini_assignment(line)
            && !inner.contains_key(&key)
            && old_inner.contains_key(&key)
        {
            remove.push(i);
        }
    }
    for i in remove.into_iter().rev() {
        lines.remove(i);
    }
}

fn splice_ini(
    lines: &mut Vec<String>,
    old_obj: &Map<String, Value>,
    new_obj: &Map<String, Value>,
) -> Option<()> {
    drop_stale_ini_lines(lines, new_obj);
    for (key, new_v) in new_obj {
        match new_v {
            Value::Object(inner) => {
                if !ini_section_reads_back(key) {
                    return None;
                }
                let old_inner = old_obj.get(key).and_then(Value::as_object);
                ensure_ini_section(lines, key);
                for (ik, iv) in inner {
                    let new_s = value_as_string(iv)?;
                    if old_inner.and_then(|m| m.get(ik)) == Some(iv) {
                        continue;
                    }
                    if !ini_key_reads_back(ik, &new_s) {
                        return None;
                    }
                    let hits = find_ini_key_lines(lines, key, ik);
                    if hits.is_empty() {
                        if let Some(end) = ini_section_end(lines, key) {
                            lines.insert(end, format!("{ik}={new_s}"));
                        }
                    } else {
                        for idx in hits {
                            lines[idx] = replace_ini_value(&lines[idx], &new_s);
                        }
                    }
                }
                if let Some(old_inner) = old_inner
                    && old_inner.keys().any(|k| !inner.contains_key(k))
                {
                    remove_omitted_ini_section_keys(lines, key, inner, old_inner);
                }
            }
            _ => {
                let new_s = value_as_string(new_v)?;
                if old_obj.get(key) == Some(new_v) {
                    continue;
                }
                if !ini_key_reads_back(key, &new_s) {
                    return None;
                }
                let hits = find_ini_global_keys(lines, key);
                if hits.is_empty() {
                    insert_ini_root_line(lines, format!("{key}={new_s}"));
                } else {
                    for idx in hits {
                        lines[idx] = replace_ini_value(&lines[idx], &new_s);
                    }
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
    let sep = ini_separator_index(trimmed)?;
    let key = trimmed[..sep].trim();
    if key.is_empty() {
        return None;
    }
    Some((
        key.to_string(),
        ini_value_and_comment(&trimmed[sep + 1..]).0,
    ))
}

/// Value, plus the inline comment suffix (including the spaces before it).
///
/// A `;` or `#` after whitespace is a comment, same as a section header.
/// A marker glued to the value (`8080;primary`, `foo#bar`) stays in the value.
fn ini_value_and_comment(raw: &str) -> (String, &str) {
    let lead = raw.len() - raw.trim_start().len();
    let body = &raw[lead..];
    if body.is_empty() {
        return (String::new(), "");
    }
    if (body.starts_with(';') || body.starts_with('#')) && lead > 0 {
        return (String::new(), raw);
    }
    // A marker at index 0 stays in the value (`#ff00aa`). A later
    // space-before-marker is still a comment (`#ff00aa ; brand`).
    let (value, comment) = split_ini_inline_comment(body);
    (value.to_string(), comment)
}

fn split_ini_inline_comment(raw: &str) -> (&str, &str) {
    let mut comment_at = None;
    for (idx, c) in raw.char_indices() {
        if (c == ';' || c == '#') && idx > 0 {
            let prev = raw[..idx].chars().next_back();
            if prev.is_some_and(|p| p.is_whitespace()) {
                comment_at = Some(idx);
                break;
            }
        }
    }
    let Some(at) = comment_at else {
        return (raw.trim_end(), "");
    };
    let mut start = at;
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

fn ini_line_section(lines: &[String], idx: usize) -> Option<String> {
    let mut cur = None;
    for line in lines.iter().take(idx + 1) {
        if let Some(name) = parse_ini_section(line.trim()) {
            cur = Some(name);
        }
    }
    cur
}

fn find_ini_key_lines(lines: &[String], section: &str, key: &str) -> Vec<usize> {
    let mut in_section = false;
    let mut hits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if let Some(name) = parse_ini_section(t) {
            in_section = name == section;
            continue;
        }
        if in_section && parse_ini_assignment(line).is_some_and(|(k, _)| k == key) {
            hits.push(i);
        }
    }
    hits
}

fn find_ini_global_keys(lines: &[String], key: &str) -> Vec<usize> {
    let mut in_section = false;
    let mut hits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if parse_ini_section(line.trim()).is_some() {
            in_section = true;
            continue;
        }
        if !in_section && parse_ini_assignment(line).is_some_and(|(k, _)| k == key) {
            hits.push(i);
        }
    }
    hits
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
    let Some(eq) = ini_separator_index(rest) else {
        return line.to_string();
    };
    let delim = rest[eq..].chars().next().unwrap_or('=');
    let key_part = &rest[..eq];
    let after = &rest[eq + delim.len_utf8()..];
    let (_, suffix) = ini_value_and_comment(after);
    let lead = after.len() - after.trim_start().len();
    let body = &after[lead..];
    // Spaces before a comment marker belong to the suffix, not the value gap.
    let raw_gap = if !suffix.is_empty() && (body.starts_with(';') || body.starts_with('#')) {
        ""
    } else {
        &after[..lead]
    };
    // `color = #ff00aa` would read back as an empty comment. Drop the gap
    // so a value that starts with `#` or `;` stays glued to the separator.
    let gap = if !raw_gap.is_empty() && (new_s.starts_with('#') || new_s.starts_with(';')) {
        ""
    } else {
        raw_gap
    };
    format!("{prefix}{key_part}{delim}{gap}{new_s}{suffix}")
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
    fn env_key_that_would_read_back_as_another_key_is_invalid_input() {
        let orig = "A=1\n";
        let old = parse_kv(orig, FileFormat::Env).unwrap();
        for key in ["A=B", "#tag", "A B"] {
            let mut new = old.clone();
            new[key] = json!("c");
            let err = serialize_kv_preserving(orig, &old, &new, FileFormat::Env).unwrap_err();
            assert!(crate::exit::is_invalid_input(&err), "{key}: {err}");
            assert!(err.to_string().contains(key), "{key}: {err}");
        }
        let err = serialize_kv(&json!({"A=B": "c"}), FileFormat::Env).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
    }

    #[test]
    fn env_value_may_contain_equals() {
        let orig = "A=1\n";
        let old = parse_kv(orig, FileFormat::Env).unwrap();
        let mut new = old.clone();
        new["A"] = json!("b=c");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Env).unwrap();
        let got = parse_kv(&out, FileFormat::Env).unwrap();
        assert_eq!(got["A"], json!("b=c"));
        assert!(got.get("b").is_none());
    }

    #[test]
    fn ini_key_that_would_read_back_as_another_key_is_invalid_input() {
        let orig = "[db]\nport=1\na=1\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        for key in ["a=b", "a:b", ";hidden", "#hidden"] {
            let mut new = old.clone();
            new["db"][key] = json!("c");
            let err = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap_err();
            assert!(crate::exit::is_invalid_input(&err), "{key}: {err}");
            assert!(err.to_string().contains(key), "{key}: {err}");
        }
        let err = serialize_kv(&json!({"db": {"a=b": "c"}}), FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        let err = serialize_kv(&json!({"a]b": {"port": "1"}}), FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("a]b"), "{err}");
    }

    #[test]
    fn ini_value_may_contain_equals() {
        let orig = "[db]\na=1\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["db"]["a"] = json!("b=c");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["db"]["a"], json!("b=c"));
        assert!(got["db"].get("b").is_none());
    }

    #[test]
    fn ini_colon_assignment_is_kept() {
        let text = "[db]\nport: 5432\nhost=localhost\n";
        let got = crate::ops::doc::parse_doc(text, &FileFormat::Ini).unwrap();
        assert_eq!(
            got,
            json!({"db": {"host": "localhost", "port": "5432"}}),
            "{got}"
        );

        let tight = parse_kv("[db]\nport:5432\nhost=localhost\n", FileFormat::Ini).unwrap();
        assert_eq!(tight["db"]["port"], json!("5432"));
        assert_eq!(tight["db"]["host"], json!("localhost"));

        let root = parse_kv("port: 5432\n[db]\nhost=localhost\n", FileFormat::Ini).unwrap();
        assert_eq!(root["port"], json!("5432"));
        assert_eq!(root["db"]["host"], json!("localhost"));
        assert!(root["db"].get("port").is_none(), "{root}");

        let named = parse_kv("[db:port]\nhost=localhost\n", FileFormat::Ini).unwrap();
        assert_eq!(named["db:port"]["host"], json!("localhost"));
        assert!(named.get("db").is_none(), "{named}");

        let spaced = parse_kv(
            "[db] ; host: primary\n# c\n; also\n\nport : 5432\nurl=http://example.com:8080\nempty:\nnote: 8080 ; primary\n",
            FileFormat::Ini,
        )
        .unwrap();
        assert_eq!(spaced["db"]["port"], json!("5432"), "{spaced}");
        assert_eq!(
            spaced["db"]["url"],
            json!("http://example.com:8080"),
            "{spaced}"
        );
        assert_eq!(spaced["db"]["empty"], json!(""), "{spaced}");
        assert_eq!(spaced["db"]["note"], json!("8080"), "{spaced}");
        assert!(spaced["db"].get("host").is_none(), "{spaced}");
        assert!(spaced.get("host").is_none(), "{spaced}");

        let first_colon = parse_kv("a:b:c\n", FileFormat::Ini).unwrap();
        assert_eq!(first_colon["a"], json!("b:c"));
        assert!(first_colon.get("b").is_none(), "{first_colon}");

        let mixed = parse_kv("a:b=c\n", FileFormat::Ini).unwrap();
        assert_eq!(mixed["a"], json!("b=c"));
        assert!(mixed.get("a:b").is_none(), "{mixed}");
        assert!(mixed.get("b").is_none(), "{mixed}");

        let orig = "[db]\nport: 5432 ; primary\nhost=localhost\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut updated = old.clone();
        updated["db"]["port"] = json!("5433");
        let out = serialize_kv_preserving(orig, &old, &updated, FileFormat::Ini).unwrap();
        assert!(out.contains("port: 5433 ; primary"), "{out:?}");
        assert!(out.contains("host=localhost"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["db"]["port"], json!("5433"));

        let mut deleted = old.clone();
        deleted["db"].as_object_mut().unwrap().remove("port");
        let out = serialize_kv_preserving(orig, &old, &deleted, FileFormat::Ini).unwrap();
        assert!(out.contains("[db]"), "{out:?}");
        assert!(out.contains("host=localhost"), "{out:?}");
        assert!(!out.contains("port"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert!(got["db"].get("port").is_none(), "{got}");
        assert_eq!(got["db"]["host"], json!("localhost"));

        let color = "[s]\ncolor: red ; name\n";
        let old = parse_kv(color, FileFormat::Ini).unwrap();
        let mut next = old.clone();
        next["s"]["color"] = json!("#ff00aa");
        let out = serialize_kv_preserving(color, &old, &next, FileFormat::Ini).unwrap();
        assert!(out.contains("color:#ff00aa ; name"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["s"]["color"], json!("#ff00aa"));

        let spaced_key = "[s]\nport : 1\n";
        let old = parse_kv(spaced_key, FileFormat::Ini).unwrap();
        let mut next = old.clone();
        next["s"]["port"] = json!("2");
        let out = serialize_kv_preserving(spaced_key, &old, &next, FileFormat::Ini).unwrap();
        assert!(out.contains("port : 2\n"), "{out:?}");
    }

    #[test]
    fn ini_line_that_is_not_an_assignment_is_a_parse_error() {
        for (src, snippet) in [
            ("[db]\nport: 5432\nnot an assignment\n", "not an assignment"),
            ("[db]\n=5432\n", "=5432"),
            ("[db]\n:5432\n", ":5432"),
            ("[db]\nport 5432\n", "port 5432"),
            ("just text\n", "just text"),
        ] {
            let err = crate::ops::doc::parse_doc(src, &FileFormat::Ini).unwrap_err();
            assert!(crate::exit::is_parse_error(&err), "{src}: {err}");
            let msg = err.to_string();
            assert!(msg.contains("line "), "{src}: {msg}");
            assert!(msg.contains(snippet), "{src}: {msg}");
        }
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
    fn ini_empty_section_is_kept() {
        let text = "[defaults]\n; later\n\n[db]\nname=app\n";
        let got = crate::ops::doc::parse_doc(text, &FileFormat::Ini).unwrap();
        assert_eq!(got, json!({"defaults": {}, "db": {"name": "app"}}), "{got}");
        let mut updated = got.clone();
        updated["db"]["name"] = json!("app2");
        let out = serialize_kv_preserving(text, &got, &updated, FileFormat::Ini).unwrap();
        assert!(out.contains("[defaults]"), "{out:?}");
        assert!(out.contains("; later"), "{out:?}");
        assert!(out.contains("name=app2"), "{out:?}");
        let again = crate::ops::doc::parse_doc(&out, &FileFormat::Ini).unwrap();
        assert_eq!(again["defaults"], json!({}), "{again}");
        assert_eq!(again["db"]["name"], json!("app2"), "{again}");

        let written = serialize_kv(&json!({"defaults": {}}), FileFormat::Ini).unwrap();
        let round = crate::ops::doc::parse_doc(&written, &FileFormat::Ini).unwrap();
        assert_eq!(round, json!({"defaults": {}}), "{written:?}");
    }

    #[test]
    fn ini_section_header_trims_surrounding_space() {
        let text = "[ database ]\nhost=localhost\n";
        let got = crate::ops::doc::parse_doc(text, &FileFormat::Ini).unwrap();
        assert_eq!(got, json!({"database": {"host": "localhost"}}), "{got}");
        let mut updated = got.clone();
        updated["database"]["host"] = json!("127.0.0.1");
        let out = serialize_kv_preserving(text, &got, &updated, FileFormat::Ini).unwrap();
        assert!(out.contains("[ database ]"), "{out:?}");
        assert!(out.contains("host=127.0.0.1"), "{out:?}");
        assert_eq!(out.matches('[').count(), 1, "{out:?}");
        let again = crate::ops::doc::parse_doc(&out, &FileFormat::Ini).unwrap();
        assert_eq!(again["database"]["host"], json!("127.0.0.1"), "{again}");

        let spaced = parse_kv("[ my section ]\nname=app\n", FileFormat::Ini).unwrap();
        assert_eq!(spaced["my section"]["name"], json!("app"), "{spaced}");

        let err = parse_kv("[   ]\nhost=1\n", FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_parse_error(&err), "{err}");
        assert!(err.to_string().contains("[   ]"), "{err}");
    }

    #[test]
    fn ini_full_write_puts_root_keys_before_sections() {
        let doc = json!({"db": {"host": "localhost"}, "theme": "dark"});
        let written = serialize_kv(&doc, FileFormat::Ini).unwrap();
        let theme_at = written.find("theme=dark").expect("theme line");
        let section_at = written.find("[db]").expect("section header");
        assert!(theme_at < section_at, "{written:?}");
        let got = crate::ops::doc::parse_doc(&written, &FileFormat::Ini).unwrap();
        assert_eq!(got, doc, "{written:?} -> {got}");
    }

    #[test]
    fn ini_update_rewrites_every_copy_of_a_key() {
        let text = "[db]\nhost=a ; old\nhost=b\n[other]\nport=1\n[db]\nhost=b\n";
        let old = parse_kv(text, FileFormat::Ini).unwrap();
        assert_eq!(old["db"]["host"], json!("b"), "{old}");
        let mut updated = old.clone();
        updated["db"]["host"] = json!("c");
        let out = serialize_kv_preserving(text, &old, &updated, FileFormat::Ini).unwrap();
        assert!(out.contains("host=c ; old"), "{out:?}");
        assert!(out.contains("[other]"), "{out:?}");
        assert!(out.contains("port=1"), "{out:?}");
        let host_lines: Vec<_> = out
            .lines()
            .filter(|line| line.trim_start().starts_with("host="))
            .collect();
        assert_eq!(host_lines.len(), 3, "{out:?}");
        assert!(host_lines.iter().all(|line| line.contains('c')), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["db"]["host"], json!("c"), "{got}");

        let root = "port=1\nport=2\n[db]\nname=app\n";
        let old = parse_kv(root, FileFormat::Ini).unwrap();
        assert_eq!(old["port"], json!("2"), "{old}");
        let mut updated = old.clone();
        updated["port"] = json!("3");
        let out = serialize_kv_preserving(root, &old, &updated, FileFormat::Ini).unwrap();
        let port_lines: Vec<_> = out
            .lines()
            .filter(|line| line.trim_start().starts_with("port="))
            .collect();
        assert_eq!(port_lines, ["port=3", "port=3"], "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["port"], json!("3"), "{got}");
        assert_eq!(got["db"]["name"], json!("app"), "{got}");
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
    fn ini_section_named_like_a_root_key_is_a_parse_error() {
        let err = parse_kv(
            "database = postgres\n[database]\nhost = localhost\n",
            FileFormat::Ini,
        )
        .unwrap_err();
        assert!(crate::exit::is_parse_error(&err), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("[database]"), "{msg}");
        assert!(msg.contains("line 2"), "{msg}");
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
    fn ini_preserving_delete_drops_a_root_key() {
        let orig = "; comment\nport=80\nhost=localhost\n\n[db]\nname=app\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new.as_object_mut().unwrap().remove("host");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("; comment\n"), "{out:?}");
        assert!(out.contains("port=80\n"), "{out:?}");
        assert!(out.contains("[db]\n"), "{out:?}");
        assert!(out.contains("name=app\n"), "{out:?}");
        assert!(!out.contains("host="), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert!(got.get("host").is_none(), "{got}");
        assert_eq!(got["port"], json!("80"));
        assert_eq!(got["db"]["name"], json!("app"));
    }

    #[test]
    fn ini_preserving_delete_drops_a_section() {
        let orig = "; comment\nport=80\nhost=localhost\n\n[db]\nname=app\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new.as_object_mut().unwrap().remove("db");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("port=80\n"), "{out:?}");
        assert!(out.contains("host=localhost\n"), "{out:?}");
        assert!(!out.contains("[db]"), "{out:?}");
        assert!(!out.contains("name="), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert!(got.get("db").is_none(), "{got}");
        assert_eq!(got["host"], json!("localhost"));
    }

    #[test]
    fn ini_preserving_section_replaced_by_scalar_round_trips() {
        let orig = "[port]\nhost=localhost\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let new = json!({"port": "80"});
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(!out.contains("[port]"), "{out:?}");
        assert!(!out.contains("host="), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["port"], json!("80"));
    }

    #[test]
    fn ini_preserving_section_to_scalar_stays_a_root_key_before_later_sections() {
        let orig = "[port]\nhost=localhost\n[db]\nport=1\nname=app\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let new = json!({"port": "80", "db": {"port": "1", "name": "app"}});
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["port"], json!("80"), "{out:?}");
        assert_eq!(got["db"]["port"], json!("1"), "{out:?}");
        assert_eq!(got["db"]["name"], json!("app"), "{out:?}");
        assert!(got["db"].get("host").is_none(), "{got}");
    }

    #[test]
    fn ini_preserving_new_root_key_is_not_swallowed_by_the_last_section() {
        let orig = "[db]\nname=app\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["theme"] = json!("dark");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["theme"], json!("dark"), "{out:?}");
        assert!(got["db"].get("theme").is_none(), "{got}");
        assert_eq!(got["db"]["name"], json!("app"));
    }

    #[test]
    fn ini_preserving_one_key_update_on_many_sections_keeps_the_others() {
        let mut orig = String::new();
        for section in 0..8 {
            orig.push_str(&format!("[s{section}]\n"));
            for key in 0..10 {
                orig.push_str(&format!("k{key}=v\n"));
            }
        }
        let old = parse_kv(&orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["s0"]["k0"] = json!("changed");
        let out = serialize_kv_preserving(&orig, &old, &new, FileFormat::Ini).unwrap();
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["s0"]["k0"], json!("changed"));
        assert_eq!(got["s7"]["k9"], json!("v"));
        assert!(got["s3"].get("k0").is_some());
    }

    #[test]
    fn ini_preserving_delete_drops_a_key_inside_a_section() {
        let orig = "[server]\nport=80\nhost=localhost\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["server"].as_object_mut().unwrap().remove("host");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("[server]\n"), "{out:?}");
        assert!(out.contains("port=80\n"), "{out:?}");
        assert!(!out.contains("host="), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["server"]["port"], json!("80"));
        assert!(got["server"].get("host").is_none(), "{got}");
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
    fn ini_value_inline_comment_is_not_part_of_the_value() {
        let got = parse_kv(
            "[s]\nport = 8080 ; primary\nhost=localhost # here\nglued=8080;primary\nhash=foo#bar\nempty= ; blank\n",
            FileFormat::Ini,
        )
        .unwrap();
        assert_eq!(got["s"]["port"], json!("8080"));
        assert_eq!(got["s"]["host"], json!("localhost"));
        assert_eq!(got["s"]["glued"], json!("8080;primary"));
        assert_eq!(got["s"]["hash"], json!("foo#bar"));
        assert_eq!(got["s"]["empty"], json!(""));
        let hex = parse_kv("[s]\ncolor=#ff00aa ; brand\n", FileFormat::Ini).unwrap();
        assert_eq!(hex["s"]["color"], json!("#ff00aa"));
    }

    #[test]
    fn ini_set_keeps_spacing_and_inline_comment() {
        let orig = "; service\n[server]\nport = 8080 ; primary\nhost = localhost\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        assert_eq!(old["server"]["port"], json!("8080"));
        let mut new = old.clone();
        new["server"]["port"] = json!("9090");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("; service\n"), "{out:?}");
        assert!(out.contains("port = 9090 ; primary\n"), "{out:?}");
        assert!(out.contains("host = localhost\n"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["server"]["port"], json!("9090"));
        assert_eq!(got["server"]["host"], json!("localhost"));
    }

    #[test]
    fn ini_set_hash_value_keeps_read_back_on_a_spaced_line() {
        let orig = "[s]\ncolor = red ; name\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        assert_eq!(old["s"]["color"], json!("red"));
        let mut new = old.clone();
        new["s"]["color"] = json!("#ff00aa");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("color =#ff00aa ; name\n"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["s"]["color"], json!("#ff00aa"));
    }

    #[test]
    fn ini_set_keeps_comment_after_a_leading_hash_value() {
        let orig = "[s]\ncolor=#ff00aa ; brand\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        assert_eq!(old["s"]["color"], json!("#ff00aa"));
        let mut new = old.clone();
        new["s"]["color"] = json!("red");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap();
        assert!(out.contains("color=red ; brand\n"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Ini).unwrap();
        assert_eq!(got["s"]["color"], json!("red"));
    }

    #[test]
    fn ini_value_with_spaced_comment_marker_is_invalid_input() {
        let orig = "[db]\nport=1\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut new = old.clone();
        new["db"]["port"] = json!("8080 ; primary");
        let err = serialize_kv_preserving(orig, &old, &new, FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("comment"), "{err}");
    }

    #[test]
    fn properties_escaped_separator_stays_in_the_key() {
        let orig = "path\\=name=value\na\\:b=c\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        assert_eq!(old["path=name"], json!("value"));
        assert_eq!(old["a:b"], json!("c"));
        assert!(old.get("path\\").is_none());
        let mut new = old.clone();
        new["path=name"] = json!("next");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Properties).unwrap();
        assert!(out.contains("path\\=name=next"), "{out:?}");
        assert!(out.contains("a\\:b=c"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["path=name"], json!("next"));
        assert_eq!(got["a:b"], json!("c"));
    }

    #[test]
    fn properties_new_key_escapes_separator_and_bang() {
        let orig = "other=1\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        let mut new = old.clone();
        new["path=name"] = json!("next");
        new["a:b"] = json!("c");
        new["!not"] = json!("1");
        new["path\\"] = json!("slash");
        new["#tag"] = json!("1");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Properties).unwrap();
        assert!(out.contains("path\\=name=next"), "{out:?}");
        assert!(out.contains("a\\:b=c"), "{out:?}");
        assert!(out.contains("\\!not=1"), "{out:?}");
        assert!(out.contains("path\\\\=slash"), "{out:?}");
        assert!(out.contains("\\#tag=1"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["other"], json!("1"));
        assert_eq!(got["path=name"], json!("next"));
        assert_eq!(got["a:b"], json!("c"));
        assert_eq!(got["!not"], json!("1"));
        assert_eq!(got["path\\"], json!("slash"));
        assert_eq!(got["#tag"], json!("1"));

        let fresh = serialize_kv_preserving(
            "",
            &json!({}),
            &json!({"path=name": "next", "!not": "1"}),
            FileFormat::Properties,
        )
        .unwrap();
        let fresh_got = parse_kv(&fresh, FileFormat::Properties).unwrap();
        assert_eq!(fresh_got["path=name"], json!("next"));
        assert_eq!(fresh_got["!not"], json!("1"));
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
    fn env_set_separates_comment_glued_to_closing_quote() {
        let cases = [
            (
                "KEY=\"two\"#note\n",
                "KEY",
                "three",
                "KEY=three #note",
                "three",
            ),
            ("export A=\"1\"#note\n", "A", "2", "export A=2 #note", "2"),
            ("KEY=\"\"#note\n", "KEY", "x", "KEY=x #note", "x"),
            (
                "KEY=\"two\"#note\n",
                "KEY",
                "a b",
                "KEY=\"a b\" #note",
                "a b",
            ),
        ];
        for (orig, key, value, expect_line, expect_val) in cases {
            let old = parse_kv(orig, FileFormat::Env).unwrap();
            let mut new = old.clone();
            new[key] = json!(value);
            let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Env).unwrap();
            assert!(out.contains(expect_line), "{orig:?} -> {out:?}");
            let got = parse_kv(&out, FileFormat::Env).unwrap();
            assert_eq!(got[key], json!(expect_val), "{orig:?} -> {out:?}");
        }
    }

    #[test]
    fn env_value_with_a_cr_is_invalid_input() {
        let orig = "KEY=1\n";
        let old = parse_kv(orig, FileFormat::Env).unwrap();
        let mut new = old.clone();
        new["KEY"] = json!("a\rpassword=owned");
        let err = serialize_kv_preserving(orig, &old, &new, FileFormat::Env).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("split"), "{err}");
    }

    #[test]
    fn env_multiline_quotes_and_quoted_tokens_round_trip() {
        let orig = "\
# keep
CERT=\"-----BEGIN CERTIFICATE-----
MIIF
-----END CERTIFICATE-----\"
PORT=1
";
        let old = parse_kv(orig, FileFormat::Env).unwrap();
        assert_eq!(
            old["CERT"],
            json!("-----BEGIN CERTIFICATE-----\nMIIF\n-----END CERTIFICATE-----"),
            "{old}"
        );
        assert!(old.get("MIIF").is_none(), "{old}");
        let mut sibling = old.clone();
        sibling["PORT"] = json!("2");
        let out = serialize_kv_preserving(orig, &old, &sibling, FileFormat::Env).unwrap();
        assert!(out.contains("# keep"), "{out:?}");
        assert!(out.contains("MIIF"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Env).unwrap();
        assert_eq!(got["CERT"], old["CERT"], "{out:?}");
        assert_eq!(got["PORT"], json!("2"), "{out:?}");

        let mut cert = old.clone();
        cert["CERT"] = json!("a\npassword=owned");
        let out = serialize_kv_preserving(orig, &old, &cert, FileFormat::Env).unwrap();
        let got = parse_kv(&out, FileFormat::Env).unwrap();
        assert_eq!(got["CERT"], json!("a\npassword=owned"), "{out:?}");
        assert!(got.get("password").is_none(), "{got}");

        let written = serialize_kv(
            &json!({"GREETING": "\"hello\"", "FLAG": "'yes'"}),
            FileFormat::Env,
        )
        .unwrap();
        let got = parse_kv(&written, FileFormat::Env).unwrap();
        assert_eq!(got["GREETING"], json!("\"hello\""), "{written:?}");
        assert_eq!(got["FLAG"], json!("'yes'"), "{written:?}");
    }

    #[test]
    fn ini_line_break_in_key_section_or_value_is_invalid_input() {
        let orig = "[db]\nport=1\n";
        let old = parse_kv(orig, FileFormat::Ini).unwrap();
        let mut updated = old.clone();
        updated["db"]["port"] = json!("1\npassword=owned");
        let err = serialize_kv_preserving(orig, &old, &updated, FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("split"), "{err}");

        let mut new_key = old.clone();
        new_key["db"]["a\nb"] = json!("c");
        let err = serialize_kv_preserving(orig, &old, &new_key, FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");

        let err = serialize_kv(&json!({"a\nb": {"port": "9"}}), FileFormat::Ini).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("a\nb"), "{err}");
    }

    #[test]
    fn properties_backslash_joins_the_next_line() {
        let text = "msg=hello\\\nworld\nport=1\n";
        let got = parse_kv(text, FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("helloworld"));
        assert_eq!(got["port"], json!("1"));
        assert!(got.get("world").is_none(), "{got}");

        let spaced = "msg=hello\\\n  world\n";
        let got = parse_kv(spaced, FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("helloworld"));

        let escaped = "msg=hello\\\\\nworld\n";
        let got = parse_kv(escaped, FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("hello\\"));
        assert!(got.get("world").is_none(), "{got}");
    }

    #[test]
    fn properties_comment_backslash_keeps_the_next_property() {
        // #2748: a comment cannot be continued. The next assignment stays.
        for src in [
            "# note\\\nport=1\n",
            "! note\\\nport=1\n",
            "   # note\\\nport=1\n",
            "# note\\\n# still\nport=1\n",
        ] {
            let got = parse_kv(src, FileFormat::Properties).unwrap();
            assert_eq!(got["port"], json!("1"), "{src:?} -> {got}");
            assert_eq!(got.as_object().unwrap().len(), 1, "{src:?} -> {got}");
        }

        // A blank continued line ends the logical line. The next assignment stays.
        let blank = "msg=hello\\\n\nport=1\n";
        let got = parse_kv(blank, FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("hello"), "{got}");
        assert_eq!(got["port"], json!("1"), "{got}");

        // A continuation line keeps a leading `#`. It is not a new comment.
        let hashed = "msg=hello\\\n#world\n";
        let got = parse_kv(hashed, FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("hello#world"), "{got}");

        // An escaped `#` is a key, so an odd trailing `\` still joins.
        let escaped_hash = "\\#tag\\\nmore=1\n";
        let got = parse_kv(escaped_hash, FileFormat::Properties).unwrap();
        assert_eq!(got["#tagmore"], json!("1"), "{got}");
        assert!(got.get("more").is_none(), "{got}");

        // No next line: drop the odd continuation backslash.
        for src in ["msg=hello\\\n", "msg=hello\\"] {
            let got = parse_kv(src, FileFormat::Properties).unwrap();
            assert_eq!(got["msg"], json!("hello"), "{src:?} -> {got}");
        }
        // An even trailing backslash is a literal backslash, including at EOF.
        let got = parse_kv("msg=hello\\\\\n", FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("hello\\"), "{got}");
        let got = parse_kv("msg=hello\\\\\\", FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("hello\\"), "{got}");
    }

    #[test]
    fn properties_newline_in_a_new_key_stays_one_assignment() {
        let orig = "port=1\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        let mut new = old.clone();
        new["a\nb"] = json!("c");
        new["a\rb"] = json!("d");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Properties).unwrap();
        assert!(out.contains("a\\nb=c"), "{out:?}");
        assert!(out.contains("a\\rb=d"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["a\nb"], json!("c"));
        assert_eq!(got["a\rb"], json!("d"));
        assert!(got.get("b").is_none(), "{got}");
    }

    #[test]
    fn properties_whitespace_separator_form_feed_and_unicode() {
        let src = "\
host localhost
tabbed\tvalue
site:localhost
spaced: localhost
equals = kept
url http://example.com
cheeses
msg=a\\fb
letter=\\u0041
face=\\uD83D\\uDE00
my\\ key=spaced key
trail=localhost \n";
        let got = parse_kv(src, FileFormat::Properties).unwrap();
        assert_eq!(got["host"], json!("localhost"), "{got}");
        assert_eq!(got["tabbed"], json!("value"), "{got}");
        assert_eq!(got["site"], json!("localhost"), "{got}");
        assert_eq!(got["spaced"], json!("localhost"), "{got}");
        assert_eq!(got["equals"], json!("kept"), "{got}");
        assert_eq!(got["url"], json!("http://example.com"), "{got}");
        assert_eq!(got["msg"], json!("a\u{000c}b"), "{got}");
        assert_eq!(got["letter"], json!("A"), "{got}");
        assert_eq!(got["face"], json!("\u{1F600}"), "{got}");
        assert_eq!(got["my key"], json!("spaced key"), "{got}");
        assert!(got.get("cheeses").is_none(), "{got}");
        assert_eq!(got["trail"], json!("localhost "), "{got}");
        assert!(got.get("localhost").is_none(), "{got}");

        let raw_ff = "host\u{000c}localhost\n";
        let got = parse_kv(raw_ff, FileFormat::Properties).unwrap();
        assert_eq!(got["host"], json!("localhost"), "{got}");

        let orig = "# keep\nhost localhost\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        let mut new = old.clone();
        new["host"] = json!("127.0.0.1");
        let out = serialize_kv_preserving(orig, &old, &new, FileFormat::Properties).unwrap();
        assert!(out.contains("# keep"), "{out:?}");
        assert!(out.contains("127.0.0.1"), "{out:?}");
        let updated = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(updated["host"], json!("127.0.0.1"), "{out:?}");

        let written = serialize_kv(
            &json!({"my key": "v", "msg": "a\u{000c}b"}),
            FileFormat::Properties,
        )
        .unwrap();
        assert!(written.contains("my\\ key=v"), "{written:?}");
        assert!(written.contains("msg=a\\fb"), "{written:?}");
        let round = parse_kv(&written, FileFormat::Properties).unwrap();
        assert_eq!(round["my key"], json!("v"), "{written:?}");
        assert_eq!(round["msg"], json!("a\u{000c}b"), "{written:?}");

        for src in [
            "msg=\\u12\n",
            "port=1\nmsg=\\u12\n",
            "msg=\\u12GG\n",
            "msg=\\uD800\n",
        ] {
            let err = parse_kv(src, FileFormat::Properties).unwrap_err();
            assert!(crate::exit::is_parse_error(&err), "{src:?}: {err}");
            assert!(err.to_string().contains("\\u"), "{src:?}: {err}");
        }
        let err = parse_kv("port=1\nmsg=\\u12\n", FileFormat::Properties).unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        assert!(err.to_string().contains("hex"), "{err}");
    }

    #[test]
    fn properties_preserving_write_keeps_a_wrapped_value() {
        let orig = "# note\nfruits apple, banana, \\\n    cantaloupe watermelon\ncolor=red\n";
        let old = parse_kv(orig, FileFormat::Properties).unwrap();
        assert_eq!(
            old["fruits"],
            json!("apple, banana, cantaloupe watermelon"),
            "{old}"
        );
        assert!(old.get("cantaloupe").is_none(), "{old}");
        let mut sibling = old.clone();
        sibling["color"] = json!("blue");
        let out = serialize_kv_preserving(orig, &old, &sibling, FileFormat::Properties).unwrap();
        assert!(out.contains("# note"), "{out:?}");
        assert!(out.contains("cantaloupe"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["fruits"], old["fruits"], "{out:?}");
        assert_eq!(got["color"], json!("blue"), "{out:?}");
        assert!(got.get("cantaloupe").is_none(), "{got}");

        let mut wrapped = old.clone();
        wrapped["fruits"] = json!("pear");
        let out = serialize_kv_preserving(orig, &old, &wrapped, FileFormat::Properties).unwrap();
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["fruits"], json!("pear"), "{out:?}");
        assert_eq!(got["color"], json!("red"), "{out:?}");
        assert!(got.get("cantaloupe").is_none(), "{got}");

        // A wrapped line that looks like another key must stay part of the value.
        let collision = "name=ada\nmsg=hello \\\nname grace\n";
        let old = parse_kv(collision, FileFormat::Properties).unwrap();
        assert_eq!(old["name"], json!("ada"), "{old}");
        assert_eq!(old["msg"], json!("hello name grace"), "{old}");
        let mut new = old.clone();
        new["name"] = json!("bea");
        let out = serialize_kv_preserving(collision, &old, &new, FileFormat::Properties).unwrap();
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["name"], json!("bea"), "{out:?}");
        assert_eq!(got["msg"], json!("hello name grace"), "{out:?}");

        // A `\u` that is only complete after the join must not drop the comment.
        let split_u = "# note\nmsg=\\u00\\\n41\nport=1\n";
        let old = parse_kv(split_u, FileFormat::Properties).unwrap();
        assert_eq!(old["msg"], json!("A"), "{old}");
        let mut new = old.clone();
        new["port"] = json!("2");
        let out = serialize_kv_preserving(split_u, &old, &new, FileFormat::Properties).unwrap();
        assert!(out.contains("# note"), "{out:?}");
        let got = parse_kv(&out, FileFormat::Properties).unwrap();
        assert_eq!(got["msg"], json!("A"), "{out:?}");
        assert_eq!(got["port"], json!("2"), "{out:?}");
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
