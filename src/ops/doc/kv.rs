//! Line-oriented `.env` / `.ini` / `.properties` parse and comment-preserving splice.

use super::FileFormat;
use serde_json::{Map, Value};

pub fn parse_kv(content: &str, format: FileFormat) -> anyhow::Result<Value> {
    match format {
        FileFormat::Env => Ok(Value::Object(parse_env(content))),
        FileFormat::Ini => Ok(parse_ini(content)),
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
    Some((key.to_string(), unquote(rest[eq + 1..].trim())))
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
    Some((key.to_string(), unquote(trimmed[sep + 1..].trim())))
}

fn parse_ini(content: &str) -> Value {
    let mut root = Map::new();
    let mut section: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(name) = parse_ini_section(trimmed) {
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
        let val = Value::String(unquote(trimmed[eq + 1..].trim()));
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
    Value::Object(root)
}

fn parse_ini_section(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() || inner.contains('[') || inner.contains(']') {
        return None;
    }
    Some(inner.to_string())
}

fn unquote(s: &str) -> String {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[s.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
        {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
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
        out.push_str(&quote_if_needed(&s));
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
                    out.push_str(&quote_if_needed(&s));
                    out.push('\n');
                }
            }
            _ => {
                let s = value_as_string(v).ok_or_else(|| crate::exit::InvalidInputError {
                    msg: format!(".ini key {k} must be a scalar or section object"),
                })?;
                out.push_str(k);
                out.push('=');
                out.push_str(&quote_if_needed(&s));
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
            lines.push(format!("{key}={}", quote_if_needed(&new_s)));
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
    let (head, _) = match style {
        KvStyle::Env => {
            let work = rest.strip_prefix("export ").unwrap_or(rest);
            let export = if rest.starts_with("export ") {
                "export "
            } else {
                ""
            };
            let eq = work.find('=').unwrap_or(work.len());
            (format!("{prefix}{export}{}=", work[..eq].trim_end()), ())
        }
        KvStyle::Properties => {
            let sep_at = rest.find(['=', ':']).unwrap_or(rest.len());
            let sep = rest.as_bytes().get(sep_at).copied().unwrap_or(b'=') as char;
            (format!("{prefix}{}{sep}", rest[..sep_at].trim_end()), ())
        }
    };
    format!("{head}{}", quote_if_needed(new_s))
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
                        lines.insert(end, format!("{ik}={}", quote_if_needed(&new_s)));
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
                    lines.push(format!("{key}={}", quote_if_needed(&new_s)));
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
    Some((key.to_string(), unquote(trimmed[eq + 1..].trim())))
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
    format!(
        "{prefix}{}={}",
        rest[..eq].trim_end(),
        quote_if_needed(new_s)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn env_parse_export_and_comment() {
        let val = parse_kv("# keep\nexport A=1\nB=two\n", FileFormat::Env).unwrap();
        assert_eq!(val["A"], json!("1"));
        assert_eq!(val["B"], json!("two"));
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
