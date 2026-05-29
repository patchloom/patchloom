#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use patchloom::selector::{eval, parse};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    selector_str: String,
    value: FuzzValue,
}

#[derive(Debug, Arbitrary)]
enum FuzzValue {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<FuzzValue>),
    Object(Vec<(String, FuzzValue)>),
}

impl FuzzValue {
    fn to_json(&self) -> serde_json::Value {
        match self {
            FuzzValue::Null => serde_json::Value::Null,
            FuzzValue::Bool(b) => serde_json::Value::Bool(*b),
            FuzzValue::Int(n) => serde_json::json!(*n),
            FuzzValue::Str(s) => serde_json::Value::String(s.clone()),
            FuzzValue::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.to_json()).collect())
            }
            FuzzValue::Object(entries) => {
                let map = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect();
                serde_json::Value::Object(map)
            }
        }
    }
}

fuzz_target!(|input: FuzzInput| {
    // Parse the selector; skip if it's not valid syntax.
    let Ok(selector) = parse(&input.selector_str) else {
        return;
    };
    let json_value = input.value.to_json();

    // eval must never panic on any (selector, value) combination.
    let _ = eval(&json_value, &selector);
});
