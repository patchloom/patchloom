use super::*;

mod basic {
    use super::*;

    #[test]
    fn version_compatibility() {
        assert!(is_compatible_version("1"));
        assert!(!is_compatible_version("0"));
        assert!(!is_compatible_version("2"));
    }

    #[test]
    fn tier_ordering() {
        assert!(Tier::Weak < Tier::Medium);
        assert!(Tier::Medium < Tier::Strong);
    }

    #[test]
    fn tier_parse_roundtrip() {
        for tier in [Tier::Weak, Tier::Medium, Tier::Strong] {
            let s = tier.to_string();
            let parsed: Tier = s.parse().unwrap();
            assert_eq!(parsed, tier);
        }
    }

    #[test]
    fn tier_parse_case_insensitive() {
        assert_eq!("WEAK".parse::<Tier>().unwrap(), Tier::Weak);
        assert_eq!("Medium".parse::<Tier>().unwrap(), Tier::Medium);
        assert_eq!("STRONG".parse::<Tier>().unwrap(), Tier::Strong);
    }

    #[test]
    fn operation_schemas_non_empty() {
        let schemas = operation_schemas().unwrap();
        assert!(!schemas.is_empty());
        // Every schema must have a name, description, and valid JSON parameters.
        for schema in &schemas {
            assert!(!schema.name.is_empty(), "schema name is empty");
            assert!(
                !schema.description.is_empty(),
                "schema {}: description is empty",
                schema.name
            );
            assert!(
                schema.parameters.is_object(),
                "schema {}: parameters is not an object",
                schema.name
            );
        }
    }

    #[test]
    fn operation_schemas_have_unique_names() {
        let schemas = operation_schemas().unwrap();
        let mut names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        let original_len = names.len();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate operation names found");
    }

    /// Every `plan::Operation` serde tag must appear in the static registry
    /// (and vice versa). Prevents schema export / agent-rules / MCP meta drift.
    #[test]
    fn registry_covers_every_operation_variant() {
        let full = operation_variant_schema("__probe_missing__");
        // Probe always fails; use the full schema via a known op instead.
        let _ = full;
        let generator = schemars::generate::SchemaSettings::default().into_generator();
        let root = generator.into_root_schema_for::<crate::plan::Operation>();
        let schema = serde_json::to_value(root).unwrap();
        let one_of = schema
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("Operation schema oneOf");
        let mut variant_names: Vec<String> = one_of
            .iter()
            .filter_map(|v| {
                v.pointer("/properties/op/const")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
            })
            .collect();
        variant_names.sort();
        variant_names.dedup();

        let mut registered: Vec<String> = registered_operation_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        registered.sort();
        registered.dedup();

        let missing_from_registry: Vec<_> = variant_names
            .iter()
            .filter(|n| !registered.contains(n))
            .collect();
        let extra_in_registry: Vec<_> = registered
            .iter()
            .filter(|n| !variant_names.contains(n))
            .collect();

        assert!(
            missing_from_registry.is_empty(),
            "Operation variants missing from OPERATION_REGISTRY: {missing_from_registry:?}"
        );
        assert!(
            extra_in_registry.is_empty(),
            "Registry entries with no Operation variant: {extra_in_registry:?}"
        );
    }

    #[test]
    fn agent_operations_catalogue_lists_registered_ops() {
        let catalogue = agent_operations_catalogue();
        assert!(catalogue.contains("## Operations (from schema registry)"));
        for name in registered_operation_names() {
            assert!(
                catalogue.contains(&format!("`{name}`")),
                "catalogue missing op {name}"
            );
        }
    }

    #[test]
    fn mcp_tool_description_includes_base_and_example() {
        let desc = mcp_tool_description("doc.set", Some("EXTRA_FRAGMENT"));
        let base = operation_description("doc.set").unwrap();
        assert!(desc.contains(base));
        assert!(desc.contains("EXTRA_FRAGMENT"));
        if let Some(ex) = operation_example_json("doc.set") {
            assert!(desc.contains(ex));
        }
    }

    #[test]
    fn weak_tier_returns_subset() {
        let all = operation_schemas().unwrap();
        let weak = operations_for_tier(Tier::Weak).unwrap();
        assert!(
            weak.len() < all.len(),
            "weak should be a proper subset of all"
        );
        for op in &weak {
            assert_eq!(
                op.min_tier,
                Tier::Weak,
                "weak tier should only contain weak ops, got {:?} for {}",
                op.min_tier,
                op.name
            );
        }
    }

    #[test]
    fn medium_tier_includes_weak() {
        let weak = operations_for_tier(Tier::Weak).unwrap();
        let medium = operations_for_tier(Tier::Medium).unwrap();
        assert!(medium.len() >= weak.len());
        let weak_names: Vec<&str> = weak.iter().map(|o| o.name.as_str()).collect();
        for name in &weak_names {
            assert!(
                medium.iter().any(|o| o.name == *name),
                "medium tier should include weak op: {name}"
            );
        }
    }

    #[test]
    fn strong_tier_returns_all() {
        let all = operation_schemas().unwrap();
        let strong = operations_for_tier(Tier::Strong).unwrap();
        assert_eq!(
            strong.len(),
            all.len(),
            "strong tier should include all operations"
        );
    }

    #[test]
    fn weak_tier_has_expected_ops() {
        let weak = operations_for_tier(Tier::Weak).unwrap();
        let names: Vec<&str> = weak.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"replace"), "weak tier must include replace");
        assert!(
            names.contains(&"file.create"),
            "weak tier must include file.create"
        );
        assert!(
            names.contains(&"file.delete"),
            "weak tier must include file.delete"
        );
        assert!(
            names.contains(&"tidy.fix"),
            "weak tier must include tidy.fix"
        );
    }

    #[test]
    fn medium_tier_has_expected_ops() {
        let medium = operations_for_tier(Tier::Medium).unwrap();
        let names: Vec<&str> = medium.iter().map(|o| o.name.as_str()).collect();
        assert!(
            names.contains(&"doc.set"),
            "medium tier must include doc.set"
        );
        assert!(
            names.contains(&"md.replace_section"),
            "medium tier must include md.replace_section"
        );
        assert!(
            names.contains(&"doc.merge"),
            "medium tier must include doc.merge"
        );
    }

    #[test]
    fn strong_tier_has_expected_ops() {
        let strong = operations_for_tier(Tier::Strong).unwrap();
        let names: Vec<&str> = strong.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"search"), "strong tier must include search");
        assert!(
            names.contains(&"doc.delete_where"),
            "strong tier must include doc.delete_where"
        );
    }

    #[test]
    fn system_prompt_for_weak_tier() {
        let prompt = system_prompt_for_tier(Tier::Weak).unwrap();
        assert!(prompt.contains("tier: weak"));
        assert!(prompt.contains("replace"));
        assert!(prompt.contains("file.create"));
        // Should NOT contain medium/strong ops.
        assert!(!prompt.contains("## `doc.set`"));
        assert!(!prompt.contains("## `search`"));
    }

    #[test]
    fn system_prompt_for_strong_tier() {
        let prompt = system_prompt_for_tier(Tier::Strong).unwrap();
        assert!(prompt.contains("tier: strong"));
        assert!(prompt.contains("replace"));
        assert!(prompt.contains("doc.set"));
        assert!(prompt.contains("search"));
    }

    #[test]
    fn plan_write_policy_schema_has_all_fields() {
        let schema = plan_write_policy_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("ensure_final_newline"));
        assert!(props.contains_key("normalize_eol"));
        assert!(props.contains_key("trim_trailing_whitespace"));
        assert!(props.contains_key("collapse_blanks"));
    }

    #[test]
    fn system_prompt_includes_write_policy() {
        let prompt = system_prompt_for_tier(Tier::Strong).unwrap();
        assert!(prompt.contains("write_policy"));
        assert!(prompt.contains("collapse_blanks"));
        assert!(prompt.contains("ensure_final_newline"));
    }

    #[test]
    fn system_prompt_includes_version() {
        let prompt = system_prompt_for_tier(Tier::Medium).unwrap();
        assert!(prompt.contains(&format!("format version: {INTENT_FORMAT_VERSION}")));
    }

    #[test]
    fn schemas_have_required_fields() {
        let schemas = operation_schemas().unwrap();
        for schema in &schemas {
            let required = schema.parameters.get("required");
            assert!(
                required.is_some(),
                "schema {}: must have 'required' field",
                schema.name
            );
        }
    }

    #[test]
    fn operation_examples_include_op_field() {
        for schema in operation_schemas().unwrap() {
            for ex in &schema.examples {
                let op = ex
                    .args
                    .get("op")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "schema {} example {:?}: missing 'op' field",
                            schema.name, ex.description
                        )
                    });
                assert_eq!(
                    op, schema.name,
                    "schema {} example {:?}: op must match operation name",
                    schema.name, ex.description
                );
            }
        }
    }

    #[test]
    fn operation_schema_serializes_to_json() {
        let schemas = operation_schemas().unwrap();
        let json = serde_json::to_string_pretty(&schemas).unwrap();
        assert!(!json.is_empty());
        // Roundtrip: deserialize back.
        let deserialized: Vec<OperationSchema> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), schemas.len());
    }

    /// Verify that every field accepted by `plan::Operation` for a given op
    /// name has a corresponding entry in the hand-written schema `properties`.
    /// This catches drift when new fields are added to the serde-based
    /// `Operation` enum but not to the JSON Schema export.
    #[test]
    fn schema_properties_cover_plan_operation_fields() {
        use crate::plan::Operation;

        // Build representative Operation values for each schema op name.
        // Only the field names matter; values are dummies.
        #[allow(unused_mut)]
        let mut ops: Vec<(&str, Operation)> = vec![
            (
                "replace",
                Operation::Replace {
                    glob: Some("*.rs".into()),
                    path: Some("f.rs".into()),
                    regex: false,
                    old: "a".into(),
                    new_text: Some("b".into()),
                    nth: Some(1),
                    insert_before: Some("x".into()),
                    insert_after: Some("y".into()),
                    case_insensitive: false,
                    multiline: false,
                    if_exists: false,
                    whole_line: false,
                    range: Some("1:10".into()),
                    word_boundary: false,
                    before_context: Some("ctx".into()),
                    after_context: Some("ctx".into()),
                    unique: false,
                },
            ),
            (
                "file.create",
                Operation::FileCreate {
                    path: "f".into(),
                    content: "c".into(),
                    force: Some(true),
                },
            ),
            (
                "file.append",
                Operation::FileAppend {
                    path: "f".into(),
                    content: "c".into(),
                },
            ),
            (
                "file.prepend",
                Operation::FilePrepend {
                    path: "f".into(),
                    content: "c".into(),
                },
            ),
            ("file.delete", Operation::FileDelete { path: "f".into() }),
            (
                "file.rename",
                Operation::FileRename {
                    from: "a".into(),
                    to: "b".into(),
                    force: true,
                },
            ),
            (
                "tidy.fix",
                Operation::TidyFix {
                    path: "f".into(),
                    ensure_final_newline: Some(true),
                    trim_trailing_whitespace: Some(true),
                    normalize_eol: Some("lf".into()),
                    collapse_blanks: None,
                    dedent: None,
                    indent: None,
                    lines: None,
                },
            ),
            (
                "doc.set",
                Operation::DocSet {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!("v"),
                },
            ),
            (
                "doc.delete",
                Operation::DocDelete {
                    path: "f".into(),
                    selector: "s".into(),
                },
            ),
            (
                "doc.merge",
                Operation::DocMerge {
                    path: "f".into(),
                    value: serde_json::json!({}),
                },
            ),
            (
                "doc.append",
                Operation::DocAppend {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!([]),
                },
            ),
            (
                "doc.prepend",
                Operation::DocPrepend {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!([]),
                },
            ),
            (
                "doc.update",
                Operation::DocUpdate {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!({}),
                },
            ),
            (
                "doc.move",
                Operation::DocMove {
                    path: "f".into(),
                    from: "a".into(),
                    to: "b".into(),
                },
            ),
            (
                "doc.ensure",
                Operation::DocEnsure {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!(1),
                },
            ),
            (
                "doc.delete_where",
                Operation::DocDeleteWhere {
                    path: "f".into(),
                    selector: "s".into(),
                    predicate: "p".into(),
                },
            ),
            (
                "md.replace_section",
                Operation::MdReplaceSection {
                    path: "f".into(),
                    heading: "h".into(),
                    content: "c".into(),
                },
            ),
            (
                "md.insert_after_heading",
                Operation::MdInsertAfterHeading {
                    path: "f".into(),
                    heading: "h".into(),
                    content: "c".into(),
                },
            ),
            (
                "md.insert_before_heading",
                Operation::MdInsertBeforeHeading {
                    path: "f".into(),
                    heading: "h".into(),
                    content: "c".into(),
                },
            ),
            (
                "md.upsert_bullet",
                Operation::MdUpsertBullet {
                    path: "f".into(),
                    heading: "h".into(),
                    bullet: "b".into(),
                },
            ),
            (
                "md.table_append",
                Operation::MdTableAppend {
                    path: "f".into(),
                    heading: "h".into(),
                    row: "r".into(),
                },
            ),
            (
                "md.move_section",
                Operation::MdMoveSection {
                    path: "f".into(),
                    heading: "h".into(),
                    to: None,
                    before: None,
                    after: None,
                },
            ),
            (
                "md.dedupe_headings",
                Operation::MdDedupeHeadings { path: "f".into() },
            ),
            (
                "md.lint_agents",
                Operation::MdLintAgents { path: "f".into() },
            ),
            (
                "patch.apply",
                Operation::PatchApply {
                    diff: "d".into(),
                    on_stale: Default::default(),
                    allow_conflicts: false,
                },
            ),
            (
                "search",
                Operation::Search {
                    path: "f".into(),
                    pattern: "p".into(),
                    regex: false,
                    case_insensitive: false,
                    multiline: false,
                    invert_match: false,
                    context: None,
                    before_context: None,
                    after_context: None,
                    assert_count: None,
                    literal: false,
                    globs: vec![],
                    max_results: 0,
                    exclude_patterns: vec![],
                    custom_ignore_filenames: vec![],
                },
            ),
            (
                "read",
                Operation::Read {
                    path: "f".into(),
                    lines: None,
                },
            ),
        ];

        #[cfg(feature = "ast")]
        {
            ops.push((
                "ast.rename",
                Operation::AstRename {
                    path: "f.rs".into(),
                    old: "old".into(),
                    new: "new".into(),
                    lang: None,
                },
            ));
            ops.push((
                "ast.replace",
                Operation::AstReplace {
                    path: "f.rs".into(),
                    symbol: "s".into(),
                    old: "a".into(),
                    new_text: "b".into(),
                    regex: false,
                    lang: None,
                },
            ));
            ops.push((
                "ast.insert",
                Operation::AstInsert {
                    path: "f.rs".into(),
                    content: "fn new() {}".into(),
                    inside: None,
                    after: Some("main".into()),
                    before: None,
                    position: None,
                    lang: None,
                },
            ));
            ops.push((
                "ast.wrap",
                Operation::AstWrap {
                    path: "f.rs".into(),
                    symbols: Some(vec!["helper".into()]),
                    lines: None,
                    wrapper: "mod internal".into(),
                    preamble: None,
                    lang: None,
                },
            ));
            ops.push((
                "ast.imports",
                Operation::AstImports {
                    path: "f.rs".into(),
                    add: Some(vec!["use std::io;".into()]),
                    remove: None,
                    dedupe: false,
                    lang: None,
                },
            ));
        }

        let schemas = operation_schemas().unwrap();
        let schema_map: std::collections::HashMap<&str, &OperationSchema> =
            schemas.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut missing: Vec<String> = Vec::new();

        for (op_name, op_value) in &ops {
            let schema = match schema_map.get(op_name) {
                Some(s) => s,
                None => {
                    missing.push(format!("{op_name}: no schema found"));
                    continue;
                }
            };
            let props = schema
                .parameters
                .get("properties")
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| {
                    panic!("schema {op_name}: parameters.properties is not an object")
                });

            // Serialize the Operation to get its field names.
            let serialized = serde_json::to_value(op_value).unwrap();
            let obj = serialized.as_object().unwrap();
            for key in obj.keys() {
                if key == "op" {
                    continue; // tag field, not a parameter
                }
                if !props.contains_key(key) {
                    missing.push(format!(
                        "{op_name}: plan.rs field '{key}' missing from schema properties"
                    ));
                }
            }
        }

        assert!(
            missing.is_empty(),
            "schema/plan drift detected:\n  {}",
            missing.join("\n  ")
        );
    }

    /// Regression: operation_variant_schema must include $defs from the root
    /// schema so that $ref pointers (e.g. OnStale in patch.apply) resolve.
    #[test]
    fn variant_schema_includes_defs_for_patch_apply() {
        let schema = operation_variant_schema("patch.apply").unwrap();
        // patch.apply references $defs/OnStale for the on_stale field.
        // Without $defs propagation, $ref pointers dangle.
        if OPERATION_FULL_SCHEMA.get("$defs").is_some() {
            assert!(
                schema.get("$defs").is_some(),
                "variant schema must include $defs from root when root has $defs"
            );
        }
    }

    /// Verify that $defs includes the referenced types.
    #[test]
    #[cfg(feature = "ast")]
    fn variant_schema_defs_include_split_target_spec() {
        let schema = operation_variant_schema("ast.split").unwrap();
        if let Some(defs) = schema.get("$defs").and_then(|d| d.as_object()) {
            assert!(
                defs.contains_key("SplitTargetSpec"),
                "$defs should contain SplitTargetSpec for ast.split"
            );
        }
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn tier_parse_invalid() {
        "unknown".parse::<Tier>().expect_err("expected error");
    }
}

mod type_extraction {
    use super::*;

    #[test]
    fn extract_type_str_scalar() {
        let schema = serde_json::json!({"type": "string"});
        assert_eq!(extract_type_str(&schema), "string");
    }

    #[test]
    fn extract_type_str_nullable_array() {
        // schemars generates {"type": ["string", "null"]} for Option<String>.
        let schema = serde_json::json!({"type": ["string", "null"]});
        let result = extract_type_str(&schema);
        assert!(
            result.contains("string") && result.contains("null"),
            "expected 'string | null' or similar, got '{result}'"
        );
    }

    #[test]
    fn extract_type_str_missing() {
        let schema = serde_json::json!({"description": "no type"});
        assert_eq!(extract_type_str(&schema), "any");
    }

    #[test]
    fn system_prompt_does_not_show_any_for_optional_params() {
        // The system prompt should show real types (string, boolean, etc.)
        // for optional parameters, not "any".
        let prompt = system_prompt_for_tier(Tier::Strong).unwrap();
        // Count occurrences of ": any" — there should be very few.
        let any_count = prompt.matches(": any").count();
        // Some fields genuinely have no type (e.g., `value` in doc.set
        // accepts any JSON), but the majority should have real types.
        // If nullable fields show "any", this count will be very high.
        assert!(
            any_count < 15,
            "too many 'any' types in system prompt ({any_count}); \
             nullable fields may not be rendering correctly"
        );
    }
}

// Static assertion: types must be Send + Sync.
const _: () = {
    fn _assert<T: Send + Sync>() {}
    let _ = _assert::<Tier>;
    let _ = _assert::<OperationSchema>;
    let _ = _assert::<OperationExample>;
};
