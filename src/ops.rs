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

    /// Serialize a value back to its original format, preserving comments and
    /// formatting for TOML and YAML files.
    ///
    /// For TOML, the original text is re-parsed with `toml_edit::DocumentMut`
    /// (which retains comments and whitespace), and only the paths that differ
    /// between `old_value` and `new_value` are updated.  Untouched keys keep
    /// their original formatting, inline comments, and section ordering.
    ///
    /// For YAML, the original text is re-parsed with `yaml_edit::Document`
    /// (a Rowan-based CST that retains comments and whitespace), and only the
    /// paths that differ between `old_value` and `new_value` are updated.
    ///
    /// JSON falls through to [`serialize_value`] (JSON has no comments).
    pub(crate) fn serialize_value_preserving(
        original_content: &str,
        old_value: &serde_json::Value,
        new_value: &serde_json::Value,
        format: &FileFormat,
    ) -> anyhow::Result<String> {
        match format {
            FileFormat::Toml => {
                let mut doc: toml_edit::DocumentMut = original_content
                    .parse()
                    .map_err(|e| anyhow::anyhow!("TOML re-parse for comment preservation: {e}"))?;
                apply_value_diff(doc.as_item_mut(), old_value, new_value);
                Ok(doc.to_string())
            }
            FileFormat::Yaml => {
                use std::str::FromStr;
                // Use YamlFile (not Document) so file-level comments that
                // precede the first mapping entry are preserved.
                let file = yaml_edit::YamlFile::from_str(original_content)
                    .map_err(|e| anyhow::anyhow!("YAML re-parse for comment preservation: {e}"))?;
                if let Some(doc) = file.document() {
                    if let Some(mapping) = doc.as_mapping() {
                        apply_yaml_mapping_diff(&mapping, old_value, new_value);
                        return Ok(file.to_string());
                    }
                }
                // Root is not a mapping (e.g. sequence-rooted YAML); fall back
                // to non-preserving serialization so mutations are not lost.
                if old_value == new_value {
                    Ok(original_content.to_string())
                } else {
                    serialize_value(new_value, format)
                }
            }
            // JSON has no comments.
            _ => serialize_value(new_value, format),
        }
    }

    // -----------------------------------------------------------------------
    // TOML comment-preserving helpers
    // -----------------------------------------------------------------------

    /// Recursively walk `old` and `new` JSON value trees and apply only the
    /// differences to the `toml_edit::Item`, preserving comments and formatting
    /// on unchanged parts.
    fn apply_value_diff(
        item: &mut toml_edit::Item,
        old: &serde_json::Value,
        new: &serde_json::Value,
    ) {
        if old == new {
            return;
        }

        match (old, new) {
            (serde_json::Value::Object(old_map), serde_json::Value::Object(new_map)) => {
                // Try to get a mutable table reference from the item.
                let table = if let Some(t) = item.as_table_mut() {
                    t
                } else if item.as_inline_table_mut().is_some() {
                    // Inline table: fall back to wholesale replacement since
                    // inline tables don't carry per-key comments.
                    *item = json_to_toml_item(new);
                    return;
                } else {
                    *item = json_to_toml_item(new);
                    return;
                };

                // Remove keys that no longer exist.
                let removed: Vec<String> = old_map
                    .keys()
                    .filter(|k| !new_map.contains_key(k.as_str()))
                    .cloned()
                    .collect();
                for k in &removed {
                    table.remove(k);
                }

                // Add new keys or recurse into changed values.
                for (key, new_val) in new_map {
                    if let Some(old_val) = old_map.get(key) {
                        if old_val != new_val {
                            if let Some(child) = table.get_mut(key) {
                                apply_value_diff(child, old_val, new_val);
                            }
                        }
                    } else {
                        table.insert(key, json_to_toml_item(new_val));
                    }
                }
            }

            (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr))
                if old_arr.len() == new_arr.len() =>
            {
                // Same-length arrays: recurse element by element.
                if let Some(arr) = item.as_array_mut() {
                    for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
                        if o != n {
                            if let Some(v) = arr.get_mut(i) {
                                *v = json_to_toml_value(n);
                            }
                        }
                    }
                } else if let Some(aot) = item.as_array_of_tables_mut() {
                    for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
                        if o != n {
                            if let Some(table_item) = aot.get_mut(i) {
                                let mut tbl_item = toml_edit::Item::Table(table_item.clone());
                                apply_value_diff(&mut tbl_item, o, n);
                                if let toml_edit::Item::Table(t) = tbl_item {
                                    *table_item = t;
                                }
                            }
                        }
                    }
                } else {
                    *item = json_to_toml_item(new);
                }
            }

            // Type changed, different-length arrays, or scalar change:
            // wholesale replacement.
            _ => {
                *item = json_to_toml_item(new);
            }
        }
    }

    /// Convert a `serde_json::Value` to a `toml_edit::Value` (scalar/array/inline-table).
    fn json_to_toml_value(val: &serde_json::Value) -> toml_edit::Value {
        match val {
            serde_json::Value::String(s) => toml_edit::Value::from(s.as_str()),
            serde_json::Value::Bool(b) => toml_edit::Value::from(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    toml_edit::Value::from(i)
                } else if let Some(f) = n.as_f64() {
                    toml_edit::Value::from(f)
                } else {
                    // u64 that doesn't fit in i64; store as float.
                    toml_edit::Value::from(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::Array(arr) => {
                let mut a = toml_edit::Array::new();
                for v in arr {
                    a.push(json_to_toml_value(v));
                }
                toml_edit::Value::Array(a)
            }
            serde_json::Value::Object(map) => {
                let mut t = toml_edit::InlineTable::new();
                for (k, v) in map {
                    t.insert(k, json_to_toml_value(v));
                }
                toml_edit::Value::InlineTable(t)
            }
            serde_json::Value::Null => {
                // TOML has no null; use empty string as fallback.
                toml_edit::Value::from("")
            }
        }
    }

    /// Convert a `serde_json::Value` to a `toml_edit::Item`.
    ///
    /// Objects become full `Table`s (not inline tables) so they render as
    /// `[section]` blocks. Arrays of objects become arrays-of-tables.
    fn json_to_toml_item(val: &serde_json::Value) -> toml_edit::Item {
        match val {
            serde_json::Value::Object(map) => {
                let mut table = toml_edit::Table::new();
                for (k, v) in map {
                    table.insert(k, json_to_toml_item(v));
                }
                toml_edit::Item::Table(table)
            }
            serde_json::Value::Array(arr)
                if !arr.is_empty() && arr.iter().all(|v| v.is_object()) =>
            {
                let mut aot = toml_edit::ArrayOfTables::new();
                for v in arr {
                    if let serde_json::Value::Object(map) = v {
                        let mut table = toml_edit::Table::new();
                        for (k, v2) in map {
                            table.insert(k, json_to_toml_item(v2));
                        }
                        aot.push(table);
                    }
                }
                toml_edit::Item::ArrayOfTables(aot)
            }
            _ => toml_edit::Item::Value(json_to_toml_value(val)),
        }
    }

    // -----------------------------------------------------------------------
    // YAML comment-preserving helpers
    // -----------------------------------------------------------------------

    /// Recursively walk `old` and `new` JSON value trees and apply only the
    /// differences to the `yaml_edit::Mapping`, preserving comments and
    /// formatting on unchanged parts.
    fn apply_yaml_mapping_diff(
        mapping: &yaml_edit::Mapping,
        old: &serde_json::Value,
        new: &serde_json::Value,
    ) {
        if old == new {
            return;
        }

        let (Some(old_map), Some(new_map)) = (old.as_object(), new.as_object()) else {
            return;
        };

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
                    // Both objects: recurse into the nested mapping.
                    (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                        if let Some(child) = mapping.get_mapping(key.as_str()) {
                            apply_yaml_mapping_diff(&child, old_val, new_val);
                        } else {
                            mapping.set(key.as_str(), json_to_yaml_mapping(new_val));
                        }
                    }
                    // Both arrays of the same length: update element by element.
                    (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr))
                        if old_arr.len() == new_arr.len() =>
                    {
                        if let Some(seq) = mapping.get_sequence(key.as_str()) {
                            apply_yaml_sequence_diff(&seq, old_arr, new_arr);
                        } else {
                            mapping.set(key.as_str(), json_to_yaml_node(new_val));
                        }
                    }
                    // Type changed, different-length arrays, or scalar change.
                    _ => {
                        mapping.set(key.as_str(), json_to_yaml_node(new_val));
                    }
                }
            } else {
                // New key: add it.
                mapping.set(key.as_str(), json_to_yaml_node(new_val));
            }
        }
    }

    /// Element-by-element diff for same-length YAML sequences.
    fn apply_yaml_sequence_diff(
        seq: &yaml_edit::Sequence,
        old_arr: &[serde_json::Value],
        new_arr: &[serde_json::Value],
    ) {
        for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
            if o == n {
                continue;
            }
            match (o, n) {
                (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                    if let Some(node) = seq.get(i) {
                        if let Some(child_mapping) = node.as_mapping() {
                            apply_yaml_mapping_diff(child_mapping, o, n);
                            continue;
                        }
                    }
                    seq.set(i, json_to_yaml_node(n));
                }
                _ => {
                    seq.set(i, json_to_yaml_node(n));
                }
            }
        }
    }

    /// Convert a `serde_json::Value` to a `yaml_edit::YamlNode` by
    /// round-tripping through `serde_yaml_ng` (for correct serialization)
    /// and `yaml_edit` (for a CST node that `Mapping::set` can accept).
    ///
    /// The value is embedded under a temporary key `__v__` so that
    /// `serde_yaml_ng` handles indentation of block sequences/mappings.
    fn json_to_yaml_node(val: &serde_json::Value) -> yaml_edit::YamlNode {
        use std::str::FromStr;
        let wrapper = serde_json::json!({ "__v__": val });
        let yaml_text = serde_yaml_ng::to_string(&wrapper).unwrap_or_else(|_| {
            // Shouldn't happen, but fall back to a null literal.
            "__v__: null\n".to_string()
        });
        let doc = yaml_edit::Document::from_str(&yaml_text)
            .expect("serde_yaml_ng output must be valid YAML");
        doc.as_mapping()
            .and_then(|m| m.get("__v__"))
            .expect("wrapper key must exist")
    }

    /// Convert a JSON object to a `yaml_edit::Mapping`.
    fn json_to_yaml_mapping(val: &serde_json::Value) -> yaml_edit::Mapping {
        let mapping = yaml_edit::Mapping::new();
        if let Some(obj) = val.as_object() {
            for (k, v) in obj {
                mapping.set(k.as_str(), json_to_yaml_node(v));
            }
        }
        mapping
    }

    pub(crate) fn parse_doc(
        content: &str,
        format: &FileFormat,
    ) -> anyhow::Result<serde_json::Value> {
        match format {
            FileFormat::Json => Ok(serde_json::from_str(content)?),
            FileFormat::Yaml => {
                let mut val: serde_json::Value = serde_yaml_ng::from_str(content)?;
                resolve_yaml_merge_keys(&mut val);
                Ok(val)
            }
            FileFormat::Toml => Ok(toml_edit::de::from_str(content)?),
        }
    }

    /// Recursively resolve YAML merge keys (`<<`) in a parsed JSON value.
    ///
    /// When `serde_yaml_ng` deserializes `<<: *anchor`, it produces a literal
    /// `"<<"` key whose value is the referenced mapping.  This function walks
    /// the tree and flattens those entries into the parent object, matching
    /// YAML merge-key semantics (existing keys take precedence).
    fn resolve_yaml_merge_keys(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                // First, recurse into all child values (including the merge value itself).
                for v in map.values_mut() {
                    resolve_yaml_merge_keys(v);
                }

                // Then resolve `<<` if present.
                if let Some(merge_val) = map.remove("<<") {
                    match merge_val {
                        serde_json::Value::Object(merged) => {
                            for (k, v) in merged {
                                map.entry(&k).or_insert(v);
                            }
                        }
                        serde_json::Value::Array(arr) => {
                            // Multiple merges: `<<: [*a, *b]` — first wins.
                            for item in arr {
                                if let serde_json::Value::Object(merged) = item {
                                    for (k, v) in merged {
                                        map.entry(&k).or_insert(v);
                                    }
                                }
                            }
                        }
                        _ => {
                            // Non-object merge value — put it back as-is.
                            map.insert("<<".to_string(), merge_val);
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    resolve_yaml_merge_keys(v);
                }
            }
            _ => {}
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

    /// Parse a `key=value` predicate and remove matching items from the array
    /// at `segments`. Returns the number of items removed.
    pub(crate) fn delete_where(
        root: &mut serde_json::Value,
        segments: &[selector::Segment],
        predicate: &str,
    ) -> anyhow::Result<usize> {
        let eq_pos = predicate
            .find('=')
            .ok_or_else(|| anyhow::anyhow!("predicate must be in key=value format"))?;
        let pred_key = &predicate[..eq_pos];
        let pred_val = &predicate[eq_pos + 1..];

        let target = navigate_mut(root, segments, false)?;
        let arr = target
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;

        let before_len = arr.len();
        arr.retain(|item| {
            item.get(pred_key)
                .map_or(true, |field| !selector::value_matches_str(field, pred_val))
        });
        Ok(before_len - arr.len())
    }

    /// Move a value from one path to another within the same document.
    /// Removes the value at `from_segments` and inserts it at `to_segments`.
    pub(crate) fn move_at_path(
        root: &mut serde_json::Value,
        from_segments: &[selector::Segment],
        to_segments: &[selector::Segment],
    ) -> anyhow::Result<()> {
        // Remove value at source path.
        let removed = {
            let last = from_segments
                .last()
                .ok_or_else(|| anyhow::anyhow!("empty from selector"))?;
            let parent_path = &from_segments[..from_segments.len() - 1];
            let parent = navigate_mut(root, parent_path, false)?;
            match last {
                selector::Segment::Key(k) => parent
                    .as_object_mut()
                    .and_then(|obj| obj.remove(k.as_str()))
                    .ok_or_else(|| anyhow::anyhow!("source key '{k}' not found"))?,
                selector::Segment::Index(i) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| anyhow::anyhow!("source parent is not an array"))?;
                    if *i < arr.len() {
                        arr.remove(*i)
                    } else {
                        anyhow::bail!("source index {i} out of bounds");
                    }
                }
                _ => anyhow::bail!("cannot move from wildcard/predicate"),
            }
        };

        // Insert at destination path.
        let last = to_segments
            .last()
            .ok_or_else(|| anyhow::anyhow!("empty to selector"))?;
        let parent_path = &to_segments[..to_segments.len() - 1];
        let parent = navigate_mut(root, parent_path, true)?;
        match last {
            selector::Segment::Key(k) => {
                parent
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("target parent is not an object"))?
                    .insert(k.clone(), removed);
            }
            selector::Segment::Index(i) => {
                let arr = parent
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("target parent is not an array"))?;
                if *i <= arr.len() {
                    arr.insert(*i, removed);
                } else {
                    anyhow::bail!("target index {i} out of bounds");
                }
            }
            _ => anyhow::bail!("cannot move to wildcard/predicate"),
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
                        let matches = item
                            .get(key.as_str())
                            .is_some_and(|field| selector::value_matches_str(field, pred_val));
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

            let src_refs: Vec<&str> = src_lines.iter().map(std::string::String::as_str).collect();
            let old_refs: Vec<&str> = old_lines.iter().map(std::string::String::as_str).collect();

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
        use crate::selector;
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
        fn yaml_merge_keys_resolved() {
            let yaml = "defaults: &d\n  timeout: 30\n  retries: 3\nstaging:\n  <<: *d\n";
            let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            assert_eq!(val["staging"]["retries"], json!(3));
            assert_eq!(val["staging"]["timeout"], json!(30));
            // The merge key itself must be removed.
            assert!(val["staging"].get("<<").is_none());
        }

        #[test]
        fn yaml_merge_key_existing_wins() {
            let yaml = "base: &b\n  x: 1\nchild:\n  <<: *b\n  x: 99\n";
            let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            assert_eq!(val["child"]["x"], json!(99));
        }

        #[test]
        fn yaml_merge_key_multiple() {
            let yaml = "a: &a\n  x: 1\nb: &b\n  y: 2\nc:\n  <<:\n    - *a\n    - *b\n";
            let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            assert_eq!(val["c"]["x"], json!(1));
            assert_eq!(val["c"]["y"], json!(2));
        }

        #[test]
        fn detect_format_unsupported() {
            assert!(detect_format("readme.txt").is_err());
        }

        // -- TOML comment preservation ----------------------------------------

        #[test]
        fn toml_comment_preserved_on_set() {
            let toml = "# top\n[server]\nhost = \"localhost\" # hostname\nport = 8080\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("server".into()),
                    selector::Segment::Key("port".into()),
                ],
                json!(9090),
            )
            .unwrap();

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            assert!(result.contains("# top"), "top comment missing");
            assert!(result.contains("# hostname"), "inline comment missing");
            assert!(result.contains("9090"), "new value missing");
            assert!(!result.contains("8080"), "old value still present");
        }

        #[test]
        fn toml_comment_preserved_on_delete() {
            let toml = "# keep this\n[section]\na = 1\nb = 2 # inline\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            // Delete key "a" from section.
            new.as_object_mut()
                .unwrap()
                .get_mut("section")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("a");

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            assert!(result.contains("# keep this"), "top comment missing");
            assert!(result.contains("# inline"), "inline comment missing");
            assert!(result.contains("b = 2"), "surviving key missing");
            assert!(!result.contains("a = 1"), "deleted key still present");
        }

        #[test]
        fn toml_section_order_preserved() {
            let toml = "[z_last]\nval = 1\n\n[a_first]\nval = 2\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("a_first".into()),
                    selector::Segment::Key("val".into()),
                ],
                json!(99),
            )
            .unwrap();

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            let z_pos = result.find("z_last").unwrap();
            let a_pos = result.find("a_first").unwrap();
            assert!(z_pos < a_pos, "section order changed: z@{z_pos} a@{a_pos}");
        }

        #[test]
        fn toml_new_key_inserted_without_breaking_comments() {
            let toml = "# config\n[pkg]\nname = \"app\"\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("pkg".into()),
                    selector::Segment::Key("version".into()),
                ],
                json!("1.0"),
            )
            .unwrap();

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            assert!(result.contains("# config"), "comment missing");
            assert!(result.contains("name = \"app\""), "existing key missing");
            assert!(result.contains("version"), "new key missing");
        }

        #[test]
        fn toml_inline_table_style_preserved() {
            let toml = "[deps]\nserde = { version = \"1\", features = [\"derive\"] }\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            // No change — verify round-trip preserves inline style.
            let result = serialize_value_preserving(toml, &old, &old, &FileFormat::Toml).unwrap();
            assert!(result.contains("serde = {"), "inline table style lost");
        }

        // -- YAML comment preservation ----------------------------------------

        #[test]
        fn yaml_comment_preserved_on_set() {
            let yaml = "# top\nserver:\n  host: localhost # hostname\n  port: 8080\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("server".into()),
                    selector::Segment::Key("port".into()),
                ],
                json!(9090),
            )
            .unwrap();

            let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
            assert!(result.contains("# top"), "top comment missing");
            assert!(result.contains("# hostname"), "inline comment missing");
            assert!(result.contains("9090"), "new value missing");
            assert!(!result.contains("8080"), "old value still present");
        }

        #[test]
        fn yaml_comment_preserved_on_delete() {
            let yaml = "# keep this\na: 1\nb: 2 # inline\nc: 3\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            // Delete key "a".
            new.as_object_mut().unwrap().remove("a");

            let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
            assert!(result.contains("# keep this"), "top comment missing");
            assert!(result.contains("# inline"), "inline comment missing");
            assert!(result.contains("b: 2"), "surviving key missing");
            assert!(!result.contains("a: 1"), "deleted key still present");
        }

        #[test]
        fn yaml_key_order_preserved() {
            let yaml = "z_last: 1\na_first: 2\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[selector::Segment::Key("a_first".into())],
                json!(99),
            )
            .unwrap();

            let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
            let z_pos = result.find("z_last").unwrap();
            let a_pos = result.find("a_first").unwrap();
            assert!(z_pos < a_pos, "key order changed: z@{z_pos} a@{a_pos}");
        }

        #[test]
        fn yaml_new_key_inserted_without_breaking_comments() {
            let yaml = "# config\nname: app\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[selector::Segment::Key("version".into())],
                json!("1.0"),
            )
            .unwrap();

            let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
            assert!(result.contains("# config"), "comment missing");
            assert!(result.contains("name: app"), "existing key missing");
            assert!(result.contains("version"), "new key missing");
        }

        #[test]
        fn yaml_noop_roundtrip_preserves_comments() {
            let yaml = "# top comment\nname: app\n# section\nserver:\n  port: 8080\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            // No change — verify round-trip preserves everything.
            let result = serialize_value_preserving(yaml, &old, &old, &FileFormat::Yaml).unwrap();
            assert_eq!(result, yaml, "no-op roundtrip should be identical");
        }

        #[test]
        fn yaml_section_comment_between_keys_preserved() {
            let yaml = "a: 1\n\n# Section B\nb: 2\n\n# Section C\nc: 3\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(&mut new, &[selector::Segment::Key("b".into())], json!(99)).unwrap();

            let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
            assert!(result.contains("# Section B"), "section B comment missing");
            assert!(result.contains("# Section C"), "section C comment missing");
            assert!(result.contains("99"), "new value missing");
            assert!(!result.contains("b: 2"), "old value still present");
        }

        #[test]
        fn yaml_sequence_root_mutation_not_lost() {
            let yaml = "- item1\n- item2\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            new.as_array_mut().unwrap().push(json!("item3"));

            let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
            assert!(result.contains("item3"), "appended item missing: {result}");
            assert!(result.contains("item1"), "item1 missing: {result}");
            assert!(result.contains("item2"), "item2 missing: {result}");
        }

        #[test]
        fn yaml_sequence_root_noop_preserves_content() {
            let yaml = "- item1\n- item2\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let result = serialize_value_preserving(yaml, &old, &old, &FileFormat::Yaml).unwrap();
            assert_eq!(result, yaml, "no-op roundtrip should be identical");
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
        fn set_at_path_simple_key() {
            let mut root = json!({"a": 1});
            let sel = crate::selector::parse("b").unwrap();
            set_at_path(&mut root, &sel, json!(2)).unwrap();
            assert_eq!(root, json!({"a": 1, "b": 2}));
        }

        #[test]
        fn set_at_path_nested_creates_intermediates() {
            let mut root = json!({});
            let sel = crate::selector::parse("a.b.c").unwrap();
            set_at_path(&mut root, &sel, json!("deep")).unwrap();
            assert_eq!(root, json!({"a": {"b": {"c": "deep"}}}));
        }

        #[test]
        fn set_at_path_array_index() {
            let mut root = json!({"items": [10, 20, 30]});
            let sel = crate::selector::parse("items[1]").unwrap();
            set_at_path(&mut root, &sel, json!(99)).unwrap();
            assert_eq!(root, json!({"items": [10, 99, 30]}));
        }

        #[test]
        fn set_at_path_out_of_bounds_index_fails() {
            let mut root = json!({"items": [1]});
            let sel = crate::selector::parse("items[5]").unwrap();
            assert!(set_at_path(&mut root, &sel, json!(99)).is_err());
        }

        #[test]
        fn set_at_path_empty_selector_fails() {
            let mut root = json!({});
            let sel: Vec<crate::selector::Segment> = vec![];
            assert!(set_at_path(&mut root, &sel, json!(1)).is_err());
        }

        #[test]
        fn delete_where_removes_matching_items() {
            let mut root = json!({"items": [{"name": "a"}, {"name": "b"}, {"name": "c"}]});
            let sel = crate::selector::parse("items").unwrap();
            let removed = delete_where(&mut root, &sel, "name=b").unwrap();
            assert_eq!(removed, 1);
            assert_eq!(root["items"].as_array().unwrap().len(), 2);
        }

        #[test]
        fn delete_where_no_match_returns_zero() {
            let mut root = json!({"items": [{"name": "a"}]});
            let sel = crate::selector::parse("items").unwrap();
            let removed = delete_where(&mut root, &sel, "name=zzz").unwrap();
            assert_eq!(removed, 0);
        }

        #[test]
        fn delete_where_invalid_predicate_fails() {
            let mut root = json!({"items": [{"name": "a"}]});
            let sel = crate::selector::parse("items").unwrap();
            assert!(delete_where(&mut root, &sel, "no-equals-sign").is_err());
        }

        #[test]
        fn delete_where_non_array_fails() {
            let mut root = json!({"items": "not-an-array"});
            let sel = crate::selector::parse("items").unwrap();
            assert!(delete_where(&mut root, &sel, "k=v").is_err());
        }

        #[test]
        fn move_at_path_renames_key() {
            let mut root = json!({"old_name": "value", "other": 1});
            let from = crate::selector::parse("old_name").unwrap();
            let to = crate::selector::parse("new_name").unwrap();
            move_at_path(&mut root, &from, &to).unwrap();
            assert_eq!(root, json!({"other": 1, "new_name": "value"}));
        }

        #[test]
        fn move_at_path_to_nested_creates_intermediates() {
            let mut root = json!({"src": 42});
            let from = crate::selector::parse("src").unwrap();
            let to = crate::selector::parse("a.b.dst").unwrap();
            move_at_path(&mut root, &from, &to).unwrap();
            assert_eq!(root, json!({"a": {"b": {"dst": 42}}}));
        }

        #[test]
        fn move_at_path_missing_source_fails() {
            let mut root = json!({"a": 1});
            let from = crate::selector::parse("nonexistent").unwrap();
            let to = crate::selector::parse("b").unwrap();
            assert!(move_at_path(&mut root, &from, &to).is_err());
        }

        #[test]
        fn move_at_path_empty_from_selector_fails() {
            let mut root = json!({"a": 1});
            let from: Vec<crate::selector::Segment> = vec![];
            let to = crate::selector::parse("b").unwrap();
            assert!(move_at_path(&mut root, &from, &to).is_err());
        }

        #[test]
        fn move_at_path_to_array_index() {
            let mut root = json!({"src": "x", "arr": [1, 2, 3]});
            let from = crate::selector::parse("src").unwrap();
            let to = crate::selector::parse("arr[1]").unwrap();
            move_at_path(&mut root, &from, &to).unwrap();
            let arr = root["arr"].as_array().unwrap();
            assert_eq!(arr.len(), 4);
            assert_eq!(arr[1], json!("x"));
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
        fn apply_hunks_two_hunks_offset_tracking() {
            // First hunk adds a line (shifting later content down), second
            // hunk must correctly account for the offset.
            let original = "a\nb\nc\nd\ne\n";
            let hunks = vec![
                Hunk {
                    old_start: 1,
                    old_count: 2,
                    new_start: 1,
                    new_count: 3,
                    lines: vec![
                        PatchLine::Context("a".into()),
                        PatchLine::Add("INSERTED".into()),
                        PatchLine::Context("b".into()),
                    ],
                },
                Hunk {
                    old_start: 4,
                    old_count: 2,
                    new_start: 5,
                    new_count: 2,
                    lines: vec![
                        PatchLine::Remove("d".into()),
                        PatchLine::Add("D".into()),
                        PatchLine::Context("e".into()),
                    ],
                },
            ];
            let result = apply_hunks(original, &hunks).unwrap();
            assert_eq!(result, "a\nINSERTED\nb\nc\nD\ne\n");
        }

        #[test]
        fn apply_hunks_pure_addition_on_empty() {
            // A patch that creates a file from scratch: old_start=0, old_count=0,
            // hunk contains only additions.
            let original = "";
            let hunks = vec![Hunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 2,
                lines: vec![
                    PatchLine::Add("new_line1".into()),
                    PatchLine::Add("new_line2".into()),
                ],
            }];
            let result = apply_hunks(original, &hunks).unwrap();
            // Empty original is treated as having a final newline, so the
            // output also gets one.
            assert_eq!(result, "new_line1\nnew_line2\n");
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
