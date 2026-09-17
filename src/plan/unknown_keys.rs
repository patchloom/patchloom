//! Warn on unknown plan / operation keys without fail-closed parse (#2486).

thread_local! {
    static UNKNOWN_PLAN_KEY_WARNINGS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Unknown-key warnings collected by the last successful plan parse.
pub fn unknown_plan_key_warnings() -> Vec<String> {
    UNKNOWN_PLAN_KEY_WARNINGS.with(|c| c.borrow().clone())
}

/// Take unknown-key warnings collected by the last successful plan parse.
pub fn take_unknown_plan_key_warnings() -> Vec<String> {
    UNKNOWN_PLAN_KEY_WARNINGS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

fn record_unknown_plan_keys(keys: Vec<String>) {
    UNKNOWN_PLAN_KEY_WARNINGS.with(|c| *c.borrow_mut() = keys);
}

const PLAN_KNOWN_KEYS: &[&str] = &[
    "version",
    "cwd",
    "write_policy",
    "strict",
    "operations",
    "ops",
    "format",
    "validate",
    "verify",
    "for_each",
];

const OP_KEY_ALIASES: &[&str] = &[
    "op", "file", "from", "to", "key", "content", "new", "name", "command",
];

fn collect_unknown_plan_keys(value: &serde_json::Value) -> Vec<String> {
    let Some(obj) = value.as_object() else {
        return Vec::new();
    };
    let mut warns = Vec::new();
    for key in obj.keys() {
        if !PLAN_KNOWN_KEYS.contains(&key.as_str()) {
            warns.push(format!("unknown plan key '{key}'"));
        }
    }
    let ops = obj
        .get("operations")
        .or_else(|| obj.get("ops"))
        .and_then(|v| v.as_array());
    if let Some(ops) = ops {
        for (i, op) in ops.iter().enumerate() {
            let Some(op_obj) = op.as_object() else {
                continue;
            };
            let op_name = op_obj.get("op").and_then(|v| v.as_str()).unwrap_or("");
            let known = known_operation_keys(op_name);
            for key in op_obj.keys() {
                if !known.iter().any(|k| k == key) && !OP_KEY_ALIASES.contains(&key.as_str()) {
                    warns.push(format!("unknown key '{key}' in operation {i} ({op_name})"));
                }
            }
        }
    }
    warns
}

fn known_operation_keys(op_name: &str) -> Vec<String> {
    let mut keys = vec!["op".to_string()];
    if let Ok(schema) = crate::schema::operation_variant_schema(op_name)
        && let Some(props) = schema.get("properties").and_then(|p| p.as_object())
    {
        keys.extend(props.keys().cloned());
    }
    keys
}

pub(super) fn note_unknown_keys_from_json_value(value: &serde_json::Value) {
    record_unknown_plan_keys(collect_unknown_plan_keys(value));
}

/// Quoted `C:\Users` is invalid YAML (`\U` escape). Peel invalid_input (#2352).
pub(super) fn map_yaml_plan_parse_error(input: &str, err: serde_yaml_ng::Error) -> anyhow::Error {
    let msg = err.to_string();
    let win_path = input
        .as_bytes()
        .windows(3)
        .any(|w| w[0].is_ascii_alphabetic() && w[1] == b':' && w[2] == b'\\');
    let l = msg.to_ascii_lowercase();
    let escape = l.contains("hexadecimal number") || l.contains("unknown escape");
    if win_path && escape {
        return crate::exit::InvalidInputError {
            msg: format!(
                "quoted YAML path looks like a Windows path with single backslashes \
                 (YAML treats \\U in C:\\Users as a unicode escape). \
                 Use forward slashes (C:/Users/...) or doubled backslashes \
                 (C:\\\\Users\\\\...): {msg}"
            ),
        }
        .into();
    }
    err.into()
}
