//! **Inventory** sourced from schema (not hand-written prose).
//!
//! The operation catalogue is built from `schema::OPERATION_REGISTRY` so it
//! cannot drift from `patchloom schema` / tier export (#1374).
//!
//! Do not move host honesty essays into `OpMeta` to shrink agent-rules.

/// Append the schema-generated operations catalogue.
pub(crate) fn append_operations_catalogue(out: &mut String) {
    // Operations catalogue generated from schema::OPERATION_REGISTRY so it
    // cannot drift from `patchloom schema` / tier export (#1374).
    out.push_str(&crate::schema::agent_operations_catalogue());
    out.push('\n');
}
