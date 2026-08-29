// size-waiver: accepted single-domain bulk (policy #1408). yaml-edit CST
// mapping/sequence diffs plus alias-to-merge line splice (#2243 / #2252).
pub(crate) fn apply_yaml_mapping_diff(
    mapping: &yaml_edit::Mapping,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> anyhow::Result<bool> {
    if old == new {
        return Ok(true);
    }

    let (Some(old_map), Some(new_map)) = (old.as_object(), new.as_object()) else {
        return Ok(true);
    };
    let mut all_applied = true;

    // Remove keys that no longer exist.
    let removed: Vec<String> = old_map
        .keys()
        .filter(|k| !new_map.contains_key(k.as_str()))
        .cloned()
        .collect();
    for k in &removed {
        mapping.remove(k.as_str());
    }

    // Add new keys or recurse into changed values.
    for (key, new_val) in new_map {
        if let Some(old_val) = old_map.get(key) {
            if old_val == new_val {
                continue;
            }
            match (old_val, new_val) {
                // Both objects: recurse using child view from get_mapping. Updates to
                // pre-existing keys inside the sub use in-place set_value (preserves
                // sibling inline comments). Brand-new keys inside may not attach on the
                // cloned sub view (structure check will catch and fallback with comments
                // preserved).
                (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                    if let Some(child) = mapping.get_mapping(key.as_str()) {
                        if !apply_yaml_mapping_diff(&child, old_val, new_val)? {
                            all_applied = false;
                        }
                    } else if !try_mapping_set(
                        mapping,
                        key.as_str(),
                        json_to_yaml_mapping(new_val)?,
                    ) {
                        all_applied = false;
                    }
                }
                // Both arrays: update via the existing sequence node to
                // preserve block/flow style and the key-value newline.
                (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr)) => {
                    if let Some(seq) = mapping.get_sequence(key.as_str()) {
                        if old_arr.len() == new_arr.len() {
                            if !apply_yaml_sequence_diff(&seq, old_arr, new_arr)? {
                                all_applied = false;
                            }
                        } else if !apply_yaml_sequence_resize(&seq, old_arr, new_arr) {
                            all_applied = false;
                        }
                    } else if !try_mapping_set(mapping, key.as_str(), json_to_yaml_node(new_val)?) {
                        all_applied = false;
                    }
                }
                // Type changed or scalar change.
                _ => {
                    // Preserve quote style when updating a scalar string.
                    if let Some(new_str) = new_val.as_str()
                        && let Some(existing) = mapping.get(key.as_str())
                        && let Some(scalar) = existing.as_scalar()
                        && scalar.is_quoted()
                    {
                        set_quoted_scalar(scalar, new_str);
                    } else if !try_mapping_set(mapping, key.as_str(), json_to_yaml_node(new_val)?) {
                        all_applied = false;
                    }
                }
            }
        } else {
            // New key: add it.
            if new_val.is_object() {
                // Follow yaml-edit's own pattern for creating nested: set empty first,
                // re-fetch the nested view (to get linked node), then populate.
                // This ensures correct block indentation and attachment in the CST.
                let empty = yaml_edit::Mapping::new();
                mapping.set(key.as_str(), &empty);
                if let Some(nested) = mapping.get_mapping(key.as_str()) {
                    if let Some(obj) = new_val.as_object() {
                        for (k, v) in obj {
                            nested.set(k.as_str(), json_to_yaml_node(v)?);
                        }
                    }
                } else {
                    mapping.set(key.as_str(), json_to_yaml_mapping(new_val)?);
                }
            } else {
                mapping.set(key.as_str(), json_to_yaml_node(new_val)?);
            }
        }
    }
    Ok(all_applied)
}

/// Element-by-element diff for same-length YAML sequences.
/// Returns `Ok(true)` if all CST changes were fully applied.
pub(super) fn apply_yaml_sequence_diff(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> anyhow::Result<bool> {
    let mut all_applied = true;
    for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
        if o == n {
            continue;
        }
        match (o, n) {
            (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                if let Some(node) = seq.get(i)
                    && let Some(child_mapping) = node.as_mapping()
                {
                    if !apply_yaml_mapping_diff(child_mapping, o, n)? {
                        all_applied = false;
                    }
                    continue;
                }
                if !try_sequence_set(seq, i, n)? {
                    all_applied = false;
                }
            }
            _ => {
                if !try_sequence_set(seq, i, n)? {
                    all_applied = false;
                }
            }
        }
    }
    Ok(all_applied)
}

/// Rewrite a quoted scalar in place, keeping the existing quote character
/// and applying the matching escape rules.
fn set_quoted_scalar(scalar: &yaml_edit::Scalar, new_str: &str) {
    let raw = scalar.value();
    let quote_char = raw.chars().next().unwrap_or('"');
    let quoted = if quote_char == '\'' {
        // Single-quoted: escape internal single quotes as ''
        format!("'{}'", new_str.replace('\'', "''"))
    } else {
        // Double-quoted: escape internal backslashes and double quotes
        format!("\"{}\"", new_str.replace('\\', "\\\\").replace('"', "\\\""))
    };
    scalar.set_value(&quoted);
}

/// yaml-edit 0.3 `Sequence::set` copies an `ALIAS` child as-is and still
/// returns true. Refuse that no-op so the caller can dump or splice.
fn try_sequence_set(
    seq: &yaml_edit::Sequence,
    index: usize,
    new_val: &serde_json::Value,
) -> anyhow::Result<bool> {
    if seq.get(index).is_some_and(|n| n.as_alias().is_some()) {
        return Ok(false);
    }
    if let Some(new_str) = new_val.as_str()
        && let Some(node) = seq.get(index)
        && let Some(scalar) = node.as_scalar()
        && scalar.is_quoted()
    {
        set_quoted_scalar(scalar, new_str);
        return Ok(true);
    }
    Ok(seq.set(index, json_to_yaml_node(new_val)?))
}

/// yaml-edit 0.3 `Mapping::set` on an existing `key: *alias` inlines the
/// replacement. Refuse that so [`rewrite_yaml_alias_object_edits`] stays
/// the only alias writer.
fn try_mapping_set(mapping: &yaml_edit::Mapping, key: &str, value: impl yaml_edit::AsYaml) -> bool {
    if mapping.get(key).is_some_and(|n| n.as_alias().is_some()) {
        return false;
    }
    mapping.set(key, value);
    true
}

/// Handle different-length array diffs while preserving comments.
///
/// Deletion is handled via targeted `Sequence::remove()` calls.
/// Growth (prepend, append, general restructuring) leaves the CST
/// unchanged; the caller in `serialize_value_preserving` handles it
/// via text-level splicing so comments are always preserved.
/// Returns `true` if the CST was successfully updated, `false` if the
/// change was too complex for the CST path (caller should flag the result
/// as needing a fallback).
fn apply_yaml_sequence_resize(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> bool {
    if new_arr.len() < old_arr.len() && try_remove_subsequence(seq, old_arr, new_arr) {
        return true;
    }
    // Growth or complex deletion: CST unchanged. Return false so the
    // caller knows a text-level fallback is needed.
    false
}

/// Try to remove elements from `seq` so that it matches `new_arr`,
/// treating `new_arr` as an ordered subsequence of `old_arr`.
///
/// Returns `true` if the removal succeeded, `false` if `new_arr` is
/// not a subsequence of `old_arr` (caller should fall back).
pub(super) fn try_remove_subsequence(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> bool {
    let new_len = new_arr.len();
    let mut remove_indices = Vec::new();
    let mut ni = 0;
    for (oi, old_item) in old_arr.iter().enumerate() {
        if ni < new_len && *old_item == new_arr[ni] {
            ni += 1;
        } else {
            remove_indices.push(oi);
        }
    }
    if ni != new_len {
        return false;
    }
    // Iterate in reverse to keep indices stable during removal.
    for &idx in remove_indices.iter().rev() {
        seq.remove(idx);
    }
    true
}

/// Rewrite `key: *alias` (or `- *alias`) lines so interior object edits keep
/// identity. yaml-edit `Mapping::set` on an inline alias either inlines
/// invalid YAML or drops sibling anchors, so this is a line splice.
///
/// Key-superset edits become `<<: *name` plus local keys. Deleting an
/// inherited key, or a non-superset replace, writes a concrete map.
pub(crate) fn rewrite_yaml_alias_object_edits(
    text: &str,
    file: &yaml_edit::YamlFile,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    let Some(doc) = file.document() else {
        return Ok(None);
    };
    let mut rewrites = Vec::new();
    let mut seq_alias_seen = std::collections::HashMap::new();
    let mut map_alias_seen = std::collections::HashMap::new();
    if let Some(mapping) = doc.as_mapping() {
        collect_mapping_alias_rewrites(
            &mapping,
            old,
            new,
            &mut seq_alias_seen,
            &mut map_alias_seen,
            &mut rewrites,
        )?;
    } else if let Some(seq) = doc.as_sequence()
        && let (Some(old_arr), Some(new_arr)) = (old.as_array(), new.as_array())
    {
        collect_sequence_alias_rewrites(
            &seq,
            old_arr,
            new_arr,
            &mut seq_alias_seen,
            &mut map_alias_seen,
            &mut rewrites,
        )?;
    }
    if rewrites.is_empty() {
        return Ok(None);
    }
    if !alias_cst_counts_match_block_lines(text, &rewrites, &seq_alias_seen, &map_alias_seen) {
        return Ok(None);
    }
    apply_alias_line_rewrites(text, &rewrites)
}

fn collect_mapping_alias_rewrites(
    mapping: &yaml_edit::Mapping,
    old: &serde_json::Value,
    new: &serde_json::Value,
    seq_alias_seen: &mut std::collections::HashMap<String, usize>,
    map_alias_seen: &mut std::collections::HashMap<(String, String), usize>,
    out: &mut Vec<AliasLineRewrite>,
) -> anyhow::Result<()> {
    let (Some(old_map), Some(new_map)) = (old.as_object(), new.as_object()) else {
        return Ok(());
    };
    // File order from the CST, then any semantic keys yaml-edit did not
    // surface. Deleted keys must still consume a file-wide `*alias` index.
    let mut keys: Vec<String> = mapping
        .keys()
        .filter_map(|k| k.as_scalar().map(|s| s.as_string()))
        .collect();
    for k in old_map.keys() {
        if !keys.iter().any(|existing| existing == k) {
            keys.push(k.clone());
        }
    }
    for key in &keys {
        let Some(old_val) = old_map.get(key) else {
            continue;
        };
        let new_val = new_map.get(key);
        if let serde_json::Value::Array(old_arr) = old_val
            && let Some(seq) = mapping.get_sequence(key.as_str())
        {
            let empty: &[serde_json::Value] = &[];
            let new_arr = new_val
                .and_then(serde_json::Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(empty);
            collect_sequence_alias_rewrites(
                &seq,
                old_arr,
                new_arr,
                seq_alias_seen,
                map_alias_seen,
                out,
            )?;
        }
        if old_val.is_object()
            && let Some(child) = mapping.get_mapping(key.as_str())
        {
            let empty_obj = serde_json::json!({});
            let child_new = match new_val {
                Some(v) if v.is_object() => v,
                _ => &empty_obj,
            };
            collect_mapping_alias_rewrites(
                &child,
                old_val,
                child_new,
                seq_alias_seen,
                map_alias_seen,
                out,
            )?;
            continue;
        }
        if let Some(node) = mapping.get(key.as_str())
            && let Some(alias) = node.as_alias()
        {
            // Flow `{key: *alias}` is not a block `key: *alias` line. Counting
            // it inflates nth so a later block site dumps or splices wrong.
            if mapping.is_flow_style() {
                continue;
            }
            let name = alias.name();
            let occ = map_alias_seen
                .entry((key.clone(), name.clone()))
                .or_insert(0);
            let this = *occ;
            *occ += 1;
            if let Some(new_val) = new_val
                && old_val != new_val
                && let Some(rewrite) = alias_object_rewrite(
                    Some(node),
                    Some(key.as_str()),
                    old_val,
                    new_val,
                    Some(this),
                )?
            {
                out.push(rewrite);
            }
        }
    }
    Ok(())
}

fn collect_sequence_alias_rewrites(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
    seq_alias_seen: &mut std::collections::HashMap<String, usize>,
    map_alias_seen: &mut std::collections::HashMap<(String, String), usize>,
    out: &mut Vec<AliasLineRewrite>,
) -> anyhow::Result<()> {
    // Index zip is only sound when lengths match. Always walk old items so
    // file-wide nth stays correct; leave length shifts to splice.
    let aligned = old_arr.len() == new_arr.len();
    let skip_flow_aliases = seq.is_flow_style();
    let empty_obj = serde_json::json!({});
    for (i, old_val) in old_arr.iter().enumerate() {
        let Some(node) = seq.get(i) else {
            continue;
        };
        if let Some(alias) = node.as_alias() {
            // Flow `[*alias]` is not a block `- *alias` line.
            if skip_flow_aliases {
                continue;
            }
            let name = alias.name();
            let occ = seq_alias_seen.entry(name.clone()).or_insert(0);
            let this = *occ;
            *occ += 1;
            if !aligned {
                continue;
            }
            let Some(new_val) = new_arr.get(i) else {
                continue;
            };
            if old_val == new_val {
                continue;
            }
            if let Some(rewrite) =
                alias_object_rewrite(Some(node), None, old_val, new_val, Some(this))?
            {
                out.push(rewrite);
            }
        } else if let Some(child) = node.as_mapping()
            && matches!(old_val, serde_json::Value::Object(_))
        {
            let child_new = if aligned {
                new_arr
                    .get(i)
                    .filter(|v| v.is_object())
                    .unwrap_or(&empty_obj)
            } else {
                &empty_obj
            };
            collect_mapping_alias_rewrites(
                child,
                old_val,
                child_new,
                seq_alias_seen,
                map_alias_seen,
                out,
            )?;
        }
    }
    Ok(())
}

fn alias_object_rewrite(
    node: Option<yaml_edit::YamlNode>,
    key: Option<&str>,
    old: &serde_json::Value,
    new: &serde_json::Value,
    seq_occurrence: Option<usize>,
) -> anyhow::Result<Option<AliasLineRewrite>> {
    let Some(alias) = node.as_ref().and_then(yaml_edit::YamlNode::as_alias) else {
        return Ok(None);
    };
    let name = alias.name();
    if !is_safe_yaml_plain_ident(&name) {
        return Ok(None);
    }
    if let Some(key) = key
        && !is_safe_yaml_plain_ident(key)
    {
        return Ok(None);
    }
    let Some(new_map) = new.as_object() else {
        return Ok(None);
    };
    let merge = old.as_object().is_some_and(|old_map| {
        old_map.keys().all(|k| new_map.contains_key(k))
            && new_map.iter().any(|(k, v)| old_map.get(k) != Some(v))
    });
    let body = if merge {
        let mut overrides = serde_json::Map::new();
        if let Some(old_map) = old.as_object() {
            for (k, v) in new_map {
                if old_map.get(k) != Some(v) {
                    overrides.insert(k.clone(), v.clone());
                }
            }
        }
        format_alias_block_body(Some(&name), &overrides)?
    } else {
        format_alias_block_body(None, new_map)?
    };
    Ok(Some(AliasLineRewrite {
        key: key.map(str::to_string),
        alias: name,
        body,
        seq_occurrence,
    }))
}

struct AliasLineRewrite {
    key: Option<String>,
    alias: String,
    body: String,
    seq_occurrence: Option<usize>,
}

/// Anchor names and mapping keys interpolated into the alias-line regex
/// must be unquoted YAML plain identifiers (no `:`, spaces, or quotes).
fn is_safe_yaml_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn format_alias_block_body(
    merge_alias: Option<&str>,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<String> {
    let mut body = String::new();
    if let Some(name) = merge_alias {
        body.push_str("<<: *");
        body.push_str(name);
        body.push('\n');
    }
    if fields.is_empty() {
        if merge_alias.is_none() {
            body.push_str("{}\n");
        }
        return Ok(body);
    }
    let yaml =
        serde_yaml_ng::to_string(&serde_json::Value::Object(fields.clone())).map_err(|e| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("YAML serialization failed: {e}"),
            })
        })?;
    for line in yaml.lines() {
        let line = line.trim_end();
        if line.is_empty() || line == "---" {
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    Ok(body)
}

/// Block-line regex indexes are only sound when every CST `*alias` is a
/// block `key: *alias` / `- *alias` line. Flow `[*alias]` / `{k: *alias}`
/// inflate CST counts; splicing would rewrite a later block site.
fn alias_cst_counts_match_block_lines(
    text: &str,
    rewrites: &[AliasLineRewrite],
    seq_alias_seen: &std::collections::HashMap<String, usize>,
    map_alias_seen: &std::collections::HashMap<(String, String), usize>,
) -> bool {
    for rewrite in rewrites {
        if let Some(key) = rewrite.key.as_deref() {
            let cst = map_alias_seen
                .get(&(key.to_string(), rewrite.alias.clone()))
                .copied()
                .unwrap_or(0);
            let Some(hits) =
                mapping_alias_line_re(key, &rewrite.alias).map(|re| re.find_iter(text).count())
            else {
                return false;
            };
            if cst != hits {
                return false;
            }
        } else {
            let cst = seq_alias_seen.get(&rewrite.alias).copied().unwrap_or(0);
            let Some(hits) =
                sequence_alias_line_re(&rewrite.alias).map(|re| re.find_iter(text).count())
            else {
                return false;
            };
            if cst != hits {
                return false;
            }
        }
    }
    true
}

fn mapping_alias_line_re(key: &str, alias: &str) -> Option<regex::Regex> {
    let key_re = regex::escape(key);
    let alias_re = regex::escape(alias);
    regex::Regex::new(&format!(
        r"(?m)^([ \t]*){key_re}:[ \t]*\*{alias_re}([ \t]*(?:#.*)?)?[ \t]*\r?$"
    ))
    .ok()
}

fn sequence_alias_line_re(alias: &str) -> Option<regex::Regex> {
    let alias_re = regex::escape(alias);
    regex::Regex::new(&format!(
        r"(?m)^([ \t]*)-[ \t]*\*{alias_re}([ \t]*(?:#.*)?)?[ \t]*\r?$"
    ))
    .ok()
}

fn apply_alias_line_rewrites(
    text: &str,
    rewrites: &[AliasLineRewrite],
) -> anyhow::Result<Option<String>> {
    let mut ordered: Vec<&AliasLineRewrite> = rewrites.iter().collect();
    // Later occurrences first so earlier `key: *alias` / `- *alias` indexes
    // stay valid after a splice.
    ordered.sort_by(|a, b| {
        match (
            a.key.as_deref(),
            b.key.as_deref(),
            a.seq_occurrence,
            b.seq_occurrence,
        ) {
            (Some(ka), Some(kb), Some(x), Some(y)) if ka == kb && a.alias == b.alias => y.cmp(&x),
            (None, None, Some(x), Some(y)) if a.alias == b.alias => y.cmp(&x),
            _ => std::cmp::Ordering::Equal,
        }
    });
    let mut out = text.to_string();
    for rewrite in ordered {
        let Some(next) = replace_unique_alias_line(&out, rewrite) else {
            return Ok(None);
        };
        out = next;
    }
    Ok(Some(out))
}

fn replace_unique_alias_line(text: &str, rewrite: &AliasLineRewrite) -> Option<String> {
    if let Some(key) = rewrite.key.as_deref() {
        let mapping_re = mapping_alias_line_re(key, &rewrite.alias)?;
        let hits: Vec<regex::Match<'_>> = mapping_re.find_iter(text).collect();
        let idx = match rewrite.seq_occurrence {
            Some(i) => i,
            None if hits.len() == 1 => 0,
            None => return None,
        };
        let hit = hits.get(idx)?;
        let caps = mapping_re.captures(hit.as_str())?;
        let indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let comment = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let replacement = with_file_eol(
            text,
            &format_block_replacement(indent, &format!("{key}:{comment}"), &rewrite.body),
        );
        return Some(splice_match(text, *hit, &replacement));
    }

    let seq_re = sequence_alias_line_re(&rewrite.alias)?;
    let hits: Vec<regex::Match<'_>> = seq_re.find_iter(text).collect();
    let idx = rewrite.seq_occurrence?;
    let hit = hits.get(idx)?;
    let caps = seq_re.captures(hit.as_str())?;
    let indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let comment = caps.get(2).map(|m| m.as_str()).unwrap_or("");
    let first = rewrite.body.lines().next().unwrap_or("");
    let rest = rewrite
        .body
        .lines()
        .skip(1)
        .map(|line| format!("{indent}  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut replacement = format!("{indent}- {first}{comment}");
    if !rest.is_empty() {
        replacement.push('\n');
        replacement.push_str(&rest);
    }
    Some(splice_match(text, *hit, &with_file_eol(text, &replacement)))
}

fn with_file_eol(file: &str, block: &str) -> String {
    if file.contains("\r\n") {
        block.replace('\n', "\r\n")
    } else {
        block.to_string()
    }
}

fn splice_match(text: &str, m: regex::Match<'_>, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..m.start()]);
    out.push_str(replacement);
    out.push_str(&text[m.end()..]);
    out
}

fn format_block_replacement(indent: &str, header: &str, body: &str) -> String {
    let mut out = format!("{indent}{header}\n");
    for line in body.lines() {
        out.push_str(indent);
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    // replace() inserts this in place of one line; drop the trailing newline
    // so we do not double the original line ending.
    out.pop();
    out
}

/// Convert a `serde_json::Value` to a `yaml_edit::YamlNode` by
/// round-tripping through `serde_yaml_ng` (for correct serialization)
/// and `yaml_edit` (for a CST node that `Mapping::set` can accept).
///
/// The value is embedded under a temporary key `__v__` so that
/// `serde_yaml_ng` handles indentation of block sequences/mappings.
fn json_to_yaml_node(val: &serde_json::Value) -> anyhow::Result<yaml_edit::YamlNode> {
    use std::str::FromStr;
    let wrapper = serde_json::json!({ "__v__": val });
    let yaml_text = serde_yaml_ng::to_string(&wrapper).map_err(|e| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("YAML serialization failed: {e}"),
        })
    })?;
    let doc = yaml_edit::Document::from_str(&yaml_text).map_err(|e| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("YAML CST re-parse failed: {e}"),
        })
    })?;
    doc.as_mapping()
        .and_then(|m| m.get("__v__"))
        .ok_or_else(|| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "YAML CST wrapper key missing".into(),
            })
        })
}

/// Convert a JSON object to a `yaml_edit::Mapping`.
fn json_to_yaml_mapping(val: &serde_json::Value) -> anyhow::Result<yaml_edit::Mapping> {
    let mapping = yaml_edit::Mapping::new();
    if let Some(obj) = val.as_object() {
        for (k, v) in obj {
            mapping.set(k.as_str(), json_to_yaml_node(v)?);
        }
    }
    Ok(mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: parse YAML text into a `yaml_edit::Document`, extract its root mapping.
    fn parse_yaml(text: &str) -> yaml_edit::Document {
        use std::str::FromStr;
        yaml_edit::Document::from_str(text).unwrap()
    }

    /// Round-trip helper: apply a mapping diff and serialize back.
    fn apply_and_serialize(yaml: &str, old: &serde_json::Value, new: &serde_json::Value) -> String {
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        apply_yaml_mapping_diff(&mapping, old, new).unwrap();
        doc.to_string()
    }

    // ---- json_to_yaml_node ----

    /// Helper: insert a json_to_yaml_node result into a pre-existing doc
    /// and return the serialized text.
    fn set_and_render(key: &str, val: &serde_json::Value) -> String {
        let doc = parse_yaml(&format!("{key}: placeholder\n"));
        let mapping = doc.as_mapping().unwrap();
        let node = json_to_yaml_node(val).unwrap();
        mapping.set(key, node);
        doc.to_string()
    }

    #[test]
    fn json_to_yaml_node_scalar() {
        let text = set_and_render("key", &json!("hello"));
        assert!(text.contains("key: hello"), "expected 'key: hello': {text}");
    }

    #[test]
    fn json_to_yaml_node_number() {
        let text = set_and_render("val", &json!(42));
        assert!(text.contains("val: 42"), "expected 'val: 42': {text}");
    }

    #[test]
    fn json_to_yaml_node_boolean() {
        let text = set_and_render("flag", &json!(true));
        assert!(text.contains("flag: true"), "expected 'flag: true': {text}");
    }

    #[test]
    fn json_to_yaml_node_array() {
        let text = set_and_render("list", &json!(["a", "b", "c"]));
        assert!(text.contains("- a"));
        assert!(text.contains("- b"));
        assert!(text.contains("- c"));
    }

    #[test]
    fn json_to_yaml_node_nested_object() {
        let text = set_and_render("outer", &json!({"inner": "value"}));
        assert!(text.contains("inner: value"));
    }

    // ---- json_to_yaml_mapping ----

    #[test]
    fn json_to_yaml_mapping_basic() {
        // json_to_yaml_mapping creates a populated Mapping. Verify by
        // setting it as a nested value and checking the rendered YAML.
        let val = json!({"alpha": 1, "beta": "two"});
        let mapping_node = json_to_yaml_mapping(&val).unwrap();
        let doc = parse_yaml("outer: placeholder\n");
        let root = doc.as_mapping().unwrap();
        root.set("outer", &mapping_node);
        let text = doc.to_string();
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
    }

    #[test]
    fn json_to_yaml_mapping_non_object_is_empty() {
        let val = json!("not an object");
        let mapping = json_to_yaml_mapping(&val).unwrap();
        // Non-object: returns empty mapping (no keys set).
        assert!(mapping.get("anything").is_none());
    }

    // ---- apply_yaml_mapping_diff ----

    #[test]
    fn mapping_diff_no_change() {
        let yaml = "key: value\n";
        let old = json!({"key": "value"});
        let new = json!({"key": "value"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert_eq!(result, yaml);
    }

    /// yaml-edit 0.3 still cannot turn `key: *alias` into `<<: *alias` via
    /// `Mapping::set`. A CST set of the expanded object inlines the map.
    /// The sibling `&shared` definition stays. Keep
    /// [`rewrite_yaml_alias_object_edits`].
    #[test]
    fn yaml_edit_set_on_pure_alias_does_not_become_merge() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let doc = parse_yaml(yaml);
        let root = doc.as_mapping().unwrap();
        let replacement = json_to_yaml_mapping(&json!({"timeout": 60, "retries": 3})).unwrap();
        root.set("service_a", &replacement);
        let result = doc.to_string();
        assert_eq!(
            result,
            "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a:\n  timeout: 60\n  retries: 3\n"
        );
    }

    /// yaml-edit 0.3 `Sequence::set` copies an ALIAS child as-is and still
    /// returns true. Product owner is [`try_sequence_set`], which refuses.
    #[test]
    fn yaml_edit_set_on_sequence_alias_item_is_noop() {
        let yaml = "shared: &shared\n  timeout: 30\nitems:\n  - *shared\n";
        let doc = parse_yaml(yaml);
        let root = doc.as_mapping().unwrap();
        let seq = root.get_sequence("items").expect("items sequence");
        let ok = seq.set(0, json_to_yaml_node(&json!({"timeout": 60})).unwrap());
        assert!(
            ok,
            "yaml-edit 0.3 Sequence::set reports success on alias item"
        );
        let result = doc.to_string();
        assert!(
            result.contains("- *shared"),
            "0.3 Sequence::set must leave the alias item:\n{result}"
        );
    }

    /// Merge writes already work on 0.3 if the replacement CST already has
    /// `<<: *name`. That is the write-side contract (jelmer/yaml-edit#39).
    #[test]
    fn yaml_edit_set_of_merge_mapping_keeps_alias() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let doc = parse_yaml(yaml);
        let root = doc.as_mapping().unwrap();
        let merge_src = parse_yaml("<<: *shared\ntimeout: 60\n");
        let merge_map = merge_src.as_mapping().expect("merge snippet is a mapping");
        root.set("service_a", &merge_map);
        let result = doc.to_string();
        assert_eq!(
            result,
            "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a:\n  <<: *shared\n  timeout: 60\n"
        );
    }

    /// `get_mapping` on `key: *alias` must not yield the `&anchor` node.
    /// If it did, `apply_yaml_mapping_diff` would mutate the definition.
    #[test]
    fn yaml_edit_get_mapping_on_alias_is_not_the_anchor() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let doc = parse_yaml(yaml);
        let root = doc.as_mapping().unwrap();
        let service_a = root.get("service_a").expect("service_a key");
        assert!(
            service_a.as_alias().is_some(),
            "service_a must still be an alias node"
        );
        assert!(
            root.get_mapping("service_a").is_none(),
            "get_mapping on a pure alias must be None; a Some view would let apply_yaml_mapping_diff recurse into &shared"
        );
    }

    /// CST fallback when rewrite is skipped: `get_mapping` is None, so
    /// `try_mapping_set` refuses. Must keep `*shared` and `&shared`.
    #[test]
    fn mapping_diff_on_alias_key_refuses_and_keeps_anchor() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let old = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 30, "retries": 3}});
        let new = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 60, "retries": 3}});
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        assert!(
            !apply_yaml_mapping_diff(&mapping, &old, &new).unwrap(),
            "alias key must be refused"
        );
        assert_eq!(
            doc.to_string(),
            "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n"
        );
    }

    #[test]
    fn mapping_diff_pure_alias_override_to_merge() {
        use std::str::FromStr;
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let old = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 30, "retries": 3}});
        let new = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 60, "retries": 3}});
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("alias line should be rewritten");
        assert_eq!(
            result,
            "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a:\n  <<: *shared\n  timeout: 60\n"
        );
    }

    /// Deleting an inherited key cannot stay a merge. The splice must
    /// expand to a concrete map; Mapping::set would inline the same
    /// semantics if rewrite returned None.
    #[test]
    fn mapping_diff_pure_alias_delete_inherited_expands() {
        use std::str::FromStr;
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\nservice_b: *shared\n";
        let old = json!({
            "shared": {"timeout": 30, "retries": 3},
            "service_a": {"timeout": 30, "retries": 3},
            "service_b": {"timeout": 30, "retries": 3}
        });
        let new = json!({
            "shared": {"timeout": 30, "retries": 3},
            "service_a": {"timeout": 30},
            "service_b": {"timeout": 30, "retries": 3}
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("delete-inherited must splice, not return None");
        assert_eq!(
            result,
            "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a:\n  timeout: 30\nservice_b: *shared\n"
        );
    }

    /// Replacing a pure alias with a non-superset map must expand. A
    /// merge would leak inherited keys; Mapping::set would also inline.
    #[test]
    fn mapping_diff_pure_alias_non_superset_replace_expands() {
        use std::str::FromStr;
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let old = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 30, "retries": 3}});
        let new = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"name": "api"}});
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("non-superset replace must splice, not return None");
        assert_eq!(
            result,
            "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a:\n  name: api\n"
        );
    }

    #[test]
    fn mapping_diff_scalar_update() {
        let yaml = "name: old\n";
        let old = json!({"name": "old"});
        let new = json!({"name": "new"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("name: new"));
    }

    #[test]
    fn mapping_diff_add_key() {
        let yaml = "existing: yes\n";
        let old = json!({"existing": "yes"});
        let new = json!({"existing": "yes", "added": "here"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("existing: yes"));
        assert!(result.contains("added: here"));
    }

    #[test]
    fn mapping_diff_remove_key() {
        let yaml = "keep: yes\nremove: me\n";
        let old = json!({"keep": "yes", "remove": "me"});
        let new = json!({"keep": "yes"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("keep: yes"));
        assert!(!result.contains("remove"));
    }

    #[test]
    fn mapping_diff_nested_object_update() {
        let yaml = "parent:\n  child: old\n";
        let old = json!({"parent": {"child": "old"}});
        let new = json!({"parent": {"child": "new"}});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("child: new"));
    }

    #[test]
    fn mapping_diff_type_change() {
        let yaml = "key: scalar\n";
        let old = json!({"key": "scalar"});
        let new = json!({"key": ["array", "now"]});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("- array"));
    }

    // ---- try_remove_subsequence ----

    #[test]
    fn try_remove_subsequence_simple_tail() {
        let yaml = "items:\n  - a\n  - b\n  - c\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a"), json!("b"), json!("c")];
        let new = vec![json!("a"), json!("b")];
        assert!(try_remove_subsequence(&seq, &old, &new));

        let result = doc.to_string();
        assert!(result.contains("- a"));
        assert!(result.contains("- b"));
        assert!(!result.contains("- c"));
    }

    #[test]
    fn try_remove_subsequence_middle() {
        let yaml = "items:\n  - x\n  - y\n  - z\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("x"), json!("y"), json!("z")];
        let new = vec![json!("x"), json!("z")];
        assert!(try_remove_subsequence(&seq, &old, &new));

        let result = doc.to_string();
        assert!(result.contains("- x"));
        assert!(!result.contains("- y"));
        assert!(result.contains("- z"));
    }

    #[test]
    fn try_remove_subsequence_not_subsequence() {
        let yaml = "items:\n  - a\n  - b\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a"), json!("b")];
        let new = vec![json!("c")]; // "c" not in old
        assert!(!try_remove_subsequence(&seq, &old, &new));
    }

    // ---- apply_yaml_sequence_diff ----

    #[test]
    fn sequence_diff_scalar_update() {
        let yaml = "list:\n  - one\n  - two\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("list").unwrap();

        let old = vec![json!("one"), json!("two")];
        let new = vec![json!("ONE"), json!("two")];
        assert!(apply_yaml_sequence_diff(&seq, &old, &new).unwrap());

        let result = doc.to_string();
        assert!(result.contains("ONE"));
        assert!(result.contains("two"));
    }

    #[test]
    fn sequence_diff_no_change() {
        let yaml = "list:\n  - a\n  - b\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("list").unwrap();

        let old = vec![json!("a"), json!("b")];
        let new = vec![json!("a"), json!("b")];
        assert!(apply_yaml_sequence_diff(&seq, &old, &new).unwrap());
    }

    /// yaml-edit 0.3 `Sequence::set` does not replace `*alias`. CST must
    /// report not-applied and leave the item so the line splice stays owner.
    #[test]
    fn sequence_diff_on_alias_item_is_not_applied() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nitems:\n  - *shared\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();
        let old = vec![json!({"timeout": 30, "retries": 3})];
        let new = vec![json!({"timeout": 60, "retries": 3})];
        assert!(
            !apply_yaml_sequence_diff(&seq, &old, &new).unwrap(),
            "alias item set must not report applied"
        );
        assert_eq!(doc.to_string(), yaml);
    }

    /// Scalar `- *alias` is the same Sequence::set no-op (non-object arm).
    #[test]
    fn sequence_diff_on_scalar_alias_item_is_not_applied() {
        let yaml = "shared: &shared hello\nitems:\n  - *shared\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();
        let old = vec![json!("hello")];
        let new = vec![json!("world")];
        assert!(
            !apply_yaml_sequence_diff(&seq, &old, &new).unwrap(),
            "scalar alias item set must not report applied"
        );
        assert_eq!(doc.to_string(), yaml);
    }

    // ---- apply_yaml_sequence_resize ----

    #[test]
    fn sequence_resize_shrink() {
        let yaml = "items:\n  - a\n  - b\n  - c\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a"), json!("b"), json!("c")];
        let new = vec![json!("a"), json!("c")];
        assert!(apply_yaml_sequence_resize(&seq, &old, &new));
        // yaml-edit leaves the next item over-indented; caller
        // `fix_yaml_block_indentation` repairs that. Lock items a, c only.
        assert_eq!(doc.to_string(), "items:\n  - a\n    - c\n");
    }

    #[test]
    fn sequence_resize_grow_returns_false() {
        let yaml = "items:\n  - a\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a")];
        let new = vec![json!("a"), json!("b")];
        // Growth is not handled by CST path, returns false.
        assert!(!apply_yaml_sequence_resize(&seq, &old, &new));
    }

    #[test]
    fn yaml_file_after_partial_alias_splice_invalid_yaml_returns_none() {
        assert!(
            super::super::yaml_file_after_partial_alias_splice("{\n").is_none(),
            "unclosed '{{' fragment must dump, not CST the pre-splice file"
        );
    }

    #[test]
    fn yaml_file_after_partial_alias_splice_valid_leftover_returns_some() {
        let spliced = "\
shared: &shared
  timeout: 30
  retries: 3
service_a:
  <<: *shared
  timeout: 60
items:
  - *shared
";
        let dummy = json!({"k": "must-not-be-used"});
        let some = super::super::yaml_file_after_partial_alias_splice(spliced);
        assert!(
            some.is_some(),
            "valid leftover alias YAML after a successful sibling splice must reparse"
        );
        let (_, cst_old) = some.unwrap();
        assert_ne!(
            cst_old, dummy,
            "cst_old must be the semantic reparse, not the pre-splice tree"
        );
        assert_eq!(cst_old["service_a"]["timeout"], json!(60));
        assert_eq!(cst_old["items"][0]["timeout"], json!(30));
    }

    #[test]
    fn yaml_file_after_partial_alias_splice_truncated_merge_returns_none() {
        assert!(
            super::super::yaml_file_after_partial_alias_splice("{\n  <<: *shared\n").is_none(),
            "truncated merge block must dump"
        );
    }

    /// yaml-edit accepts a dangling `*missing` alias; serde_yaml_ng does not.
    /// The helper must dump instead of CST-ing `old_value`.
    #[test]
    fn yaml_file_after_partial_alias_splice_yaml_edit_ok_serde_fail_returns_none() {
        use std::str::FromStr;
        let spliced = "key: *missing\n";
        assert!(
            yaml_edit::YamlFile::from_str(spliced).is_ok(),
            "fixture must be accepted by yaml-edit"
        );
        assert!(
            serde_yaml_ng::from_str::<serde_json::Value>(spliced).is_err(),
            "fixture must be rejected by serde_yaml_ng"
        );
        assert!(
            super::super::yaml_file_after_partial_alias_splice(spliced).is_none(),
            "yaml-edit-ok / serde-fail leftover must dump, not Some(old_value)"
        );
    }

    /// Leftover `key: *alias` that the line splice skips (unsafe key) must
    /// not be inlined by Mapping::set.
    #[test]
    fn try_mapping_set_refuses_existing_alias_key() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\n\"foo:bar\": *shared\n";
        let old = json!({
            "shared": {"timeout": 30, "retries": 3},
            "foo:bar": {"timeout": 30, "retries": 3}
        });
        let new = json!({
            "shared": {"timeout": 30, "retries": 3},
            "foo:bar": {"timeout": 60, "retries": 3}
        });
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let applied = apply_yaml_mapping_diff(&mapping, &old, &new).unwrap();
        assert!(
            !applied,
            "Mapping::set on an existing alias key must be refused"
        );
        let result = doc.to_string();
        assert!(
            result.contains("*shared"),
            "leftover alias must stay *shared, not inline:\n{result}"
        );
        assert!(
            !result.contains("timeout: 60"),
            "refused alias set must not inline the expanded mapping:\n{result}"
        );
    }

    /// Two same-key `*shared` mapping sites. Deleting the first must still
    /// consume its file-wide nth so the remaining site is rewritten.
    #[test]
    fn collect_mapping_alias_rewrites_counts_deleted_keys() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
left:
  cfg: *shared
right:
  cfg: *shared
";
        let old = json!({
            "shared": {"timeout": 30, "retries": 3},
            "left": {"cfg": {"timeout": 30, "retries": 3}},
            "right": {"cfg": {"timeout": 30, "retries": 3}}
        });
        let new = json!({
            "shared": {"timeout": 30, "retries": 3},
            "right": {"cfg": {"timeout": 60, "retries": 3}}
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("remaining mapping alias site must be rewritten");
        assert!(
            result.contains("left:\n  cfg: *shared"),
            "deleted first site must keep *alias identity, not be rewritten:\n{result}"
        );
        assert!(
            !result.contains("right:\n  cfg: *shared"),
            "remaining site must be rewritten, not left as the first hit:\n{result}"
        );
        assert!(
            result.contains("timeout: 60"),
            "remaining site must carry the edited value:\n{result}"
        );
    }

    /// Mixed flow `[*shared]` and later block `- *shared`. Editing the
    /// flow site is CST occ=0; the line splice only matches block lines,
    /// so it must dump instead of rewriting the unedited block item.
    #[test]
    fn rewrite_mixed_flow_and_block_alias_does_not_rewrite_unedited_block() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
flow: [*shared]
block:
  - *shared
";
        let old = json!({
            "shared": {"timeout": 30},
            "flow": [{"timeout": 30}],
            "block": [{"timeout": 30}]
        });
        let new = json!({
            "shared": {"timeout": 30},
            "flow": [{"timeout": 60}],
            "block": [{"timeout": 30}]
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new).unwrap();
        match result {
            None => {}
            Some(text) => {
                assert!(
                    text.lines().any(|l| l.trim() == "- *shared"),
                    "unedited block site must stay - *shared:\n{text}"
                );
                assert!(
                    !text.contains("- <<: *shared"),
                    "must not rewrite the unedited block site into a merge:\n{text}"
                );
            }
        }
    }

    /// Probe: flow `{cfg: *shared}` plus later block `cfg: *shared`.
    #[test]
    fn rewrite_mixed_flow_mapping_and_block_alias_does_not_rewrite_unedited_block() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
flow: {cfg: *shared}
block:
  cfg: *shared
";
        let old = json!({
            "shared": {"timeout": 30},
            "flow": {"cfg": {"timeout": 30}},
            "block": {"cfg": {"timeout": 30}}
        });
        let new = json!({
            "shared": {"timeout": 30},
            "flow": {"cfg": {"timeout": 60}},
            "block": {"cfg": {"timeout": 30}}
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new).unwrap();
        match result {
            None => {}
            Some(text) => {
                assert!(
                    text.contains("cfg: *shared"),
                    "unedited block mapping site must stay cfg: *shared:\n{text}"
                );
                assert!(
                    !text.contains("<<: *shared"),
                    "must not rewrite the unedited block mapping site into a merge:\n{text}"
                );
            }
        }
    }

    /// Mixed flow `{cfg: *shared}` plus later block `cfg: *shared`. Editing
    /// the block site must splice only that line.
    #[test]
    fn rewrite_mixed_flow_mapping_block_edit_splices_block_only() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
flow: {cfg: *shared}
block:
  cfg: *shared
";
        let old = json!({
            "shared": {"timeout": 30},
            "flow": {"cfg": {"timeout": 30}},
            "block": {"cfg": {"timeout": 30}}
        });
        let new = json!({
            "shared": {"timeout": 30},
            "flow": {"cfg": {"timeout": 30}},
            "block": {"cfg": {"timeout": 60}}
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("block-site edit must splice, not dump");
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
flow: {cfg: *shared}
block:
  cfg:
    <<: *shared
    timeout: 60
"
        );
    }

    /// Mixed flow `[*shared]` plus later block `- *shared`. Editing the
    /// block item must splice only that line.
    #[test]
    fn rewrite_mixed_flow_sequence_block_edit_splices_block_only() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
flow: [*shared]
block:
  - *shared
";
        let old = json!({
            "shared": {"timeout": 30},
            "flow": [{"timeout": 30}],
            "block": [{"timeout": 30}]
        });
        let new = json!({
            "shared": {"timeout": 30},
            "flow": [{"timeout": 30}],
            "block": [{"timeout": 60}]
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("block-site edit must splice, not dump");
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
flow: [*shared]
block:
  - <<: *shared
    timeout: 60
"
        );
    }

    /// Pure prepend must not emit a sequence alias rewrite (zip would treat
    /// the inserted object as an edit of `- *shared`).
    #[test]
    fn rewrite_sequence_alias_pure_prepend_returns_none() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
items:
  - *shared  # inherited
";
        let old = json!({
            "shared": {"timeout": 30},
            "items": [{"timeout": 30}]
        });
        let new = json!({
            "shared": {"timeout": 30},
            "items": [{"name": "x"}, {"timeout": 30}]
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new).unwrap();
        assert!(
            result.is_none(),
            "pure prepend must not rewrite - *shared, got {result:?}"
        );
    }

    /// Shrink-first + edit remaining must not zip `*gone` with the
    /// surviving object (that would rewrite `*gone` as if it were `*keep`).
    #[test]
    fn rewrite_sequence_alias_shrink_first_does_not_rewrite_removed_item() {
        use std::str::FromStr;
        let yaml = "\
gone: &gone
  timeout: 10
keep: &keep
  timeout: 30
items:
  - *gone
  - *keep
";
        let old = json!({
            "gone": {"timeout": 10},
            "keep": {"timeout": 30},
            "items": [{"timeout": 10}, {"timeout": 30}]
        });
        let new = json!({
            "gone": {"timeout": 10},
            "keep": {"timeout": 30},
            "items": [{"timeout": 60}]
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new).unwrap();
        match result {
            None => {}
            Some(text) => {
                assert!(
                    text.contains("- *gone"),
                    "removed first alias must keep - *gone identity:\n{text}"
                );
                assert!(
                    !text.contains("<<: *gone"),
                    "must not rewrite *gone as if it were the remaining edit:\n{text}"
                );
            }
        }
    }

    /// Prepend on sequence A must still count its `- *shared` so editing
    /// sequence B rewrites the second site, not the first.
    #[test]
    fn rewrite_sequence_alias_prepend_does_not_steal_sibling_nth() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
first:
  - *shared
second:
  - *shared
";
        let old = json!({
            "shared": {"timeout": 30},
            "first": [{"timeout": 30}],
            "second": [{"timeout": 30}]
        });
        let new = json!({
            "shared": {"timeout": 30},
            "first": [{"name": "x"}, {"timeout": 30}],
            "second": [{"timeout": 60}]
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("aligned sibling edit must splice");
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
first:
  - *shared
second:
  - <<: *shared
    timeout: 60
"
        );
    }

    /// Insert-middle + edit later `- *shared` must not zip the later alias
    /// onto the inserted object.
    #[test]
    fn rewrite_sequence_alias_insert_middle_does_not_rewrite_later_alias() {
        use std::str::FromStr;
        let yaml = "\
shared: &shared
  timeout: 30
items:
  - name: a
  - *shared
";
        let old = json!({
            "shared": {"timeout": 30},
            "items": [{"name": "a"}, {"timeout": 30}]
        });
        let new = json!({
            "shared": {"timeout": 30},
            "items": [{"name": "a"}, {"name": "mid"}, {"timeout": 60}]
        });
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new).unwrap();
        match result {
            None => {}
            Some(text) => {
                assert!(
                    text.lines().any(|l| l.trim() == "- *shared"),
                    "later - *shared must stay an alias:\n{text}"
                );
            }
        }
    }

    #[test]
    fn mapping_diff_remove_first_nested_key() {
        let yaml = "app:\n  name: \"my-app\"\n  version: \"1.0.0\"\n  enabled: \"true\"\n  port: \"8080\"\n";
        let old = json!({"app": {"name": "my-app", "version": "1.0.0", "enabled": "true", "port": "8080"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("app")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("name");

        let result = apply_and_serialize(yaml, &old, &new);
        // First remaining key is over-indented (CST remove); caller
        // `fix_yaml_block_indentation` repairs that. Lock remaining keys.
        assert_eq!(
            result,
            "app:\n    version: \"1.0.0\"\n  enabled: \"true\"\n  port: \"8080\"\n"
        );
    }

    #[test]
    fn mapping_diff_remove_middle_nested() {
        let yaml = "app:\n  name: \"my-app\"\n  version: \"1.0.0\"\n  port: \"8080\"\n";
        let old = json!({"app": {"name": "my-app", "version": "1.0.0", "port": "8080"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("app")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("version");

        let result = apply_and_serialize(yaml, &old, &new);
        assert!(!result.contains("version"));
        // CST preserves quotes on untouched values (indentation may be
        // wrong; fixed by fix_yaml_block_indentation in the caller).
        assert!(result.contains("\"my-app\""));
        assert!(result.contains("\"8080\""));
    }

    #[test]
    fn scalar_set_preserves_double_quote_style() {
        let yaml = "name: \"John Doe\"\nage: 30\n";
        let old = json!({"name": "John Doe", "age": 30});
        let new = json!({"name": "Jane Doe", "age": 30});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("\"Jane Doe\""),
            "double quotes not preserved: {result}"
        );
    }

    #[test]
    fn sequence_scalar_set_preserves_double_quote_style() {
        let yaml = "items:\n  - \"foo\"\n";
        let old = json!({"items": ["foo"]});
        let new = json!({"items": ["bar"]});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("- \"bar\""),
            "double quotes not preserved on sequence item: {result}"
        );
    }

    #[test]
    fn scalar_set_preserves_single_quote_style() {
        let yaml = "path: '/usr/local'\nname: plain\n";
        let old = json!({"path": "/usr/local", "name": "plain"});
        let new = json!({"path": "/opt/bin", "name": "plain"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("'/opt/bin'"),
            "single quotes not preserved: {result}"
        );
    }

    #[test]
    fn scalar_set_preserves_plain_style() {
        let yaml = "name: Alice\nage: 30\n";
        let old = json!({"name": "Alice", "age": 30});
        let new = json!({"name": "Bob", "age": 30});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("name: Bob"),
            "plain style should not add quotes: {result}"
        );
        assert!(
            !result.contains("\"Bob\"") && !result.contains("'Bob'"),
            "plain value should not get quoted: {result}"
        );
    }
}
