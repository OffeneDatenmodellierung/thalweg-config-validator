//! Produces a human/DDL-ish rendering of each real (non-virtual, valid)
//! table's inferred schema. Deliberately NOT tied to a specific SQL dialect
//! (Postgres vs Delta/zerobus) - per the v1 scope decision, sink-based
//! engine resolution is out of scope, so this emits a generic, descriptive
//! DDL rather than a guaranteed-runnable one for either backend. Revisit
//! once/if per-table engine resolution (option b from the earlier scope
//! discussion) is wanted.

use crate::lineage_engine::LineageNode;
use datafusion::arrow::datatypes::DataType;

pub fn arrow_type_to_generic_sql(dt: &DataType) -> String {
    match dt {
        DataType::Utf8 | DataType::LargeUtf8 => "VARCHAR".to_string(),
        DataType::Boolean => "BOOLEAN".to_string(),
        DataType::Int32 => "INT".to_string(),
        DataType::Int64 => "BIGINT".to_string(),
        DataType::Float64 => "DOUBLE".to_string(),
        DataType::Float32 => "FLOAT".to_string(),
        DataType::Binary | DataType::LargeBinary => "VARBINARY".to_string(),
        DataType::Timestamp(_, _) => "TIMESTAMP".to_string(),
        other => format!("/* unmapped arrow type: {other:?} */ VARCHAR"),
    }
}

/// Only emits DDL for nodes with a clean_table (i.e. real, persisted
/// tables) and a successfully inferred output schema. Virtual nodes and
/// nodes that failed validation return None.
pub fn emit_ddl(node: &LineageNode) -> Option<String> {
    let table_name = node.clean_table.as_ref()?;
    let schema = node.output_schema.as_ref()?;

    let field_lines: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| {
            let sql_type = arrow_type_to_generic_sql(f.data_type());
            let nullability = if f.is_nullable() { "" } else { " NOT NULL" };
            format!("  {} {}{}", f.name(), sql_type, nullability)
        })
        .collect();

    Some(format!(
        "CREATE TABLE {table_name} (\n{}\n);",
        field_lines.join(",\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::datatypes::{Field, Schema};
    use crate::lineage_engine::Banner;
    use std::sync::Arc;

    #[test]
    fn emits_ddl_for_real_table_with_schema() {
        let node = LineageNode {
            name: "orders".to_string(),
            is_primary: false,
            input: Some("prepared".to_string()),
            clean_table: Some("orders_clean".to_string()),
            quarantine_table: None,
            is_virtual: false,
            banner: Banner::Green,
            output_schema: Some(Arc::new(Schema::new(vec![
                Field::new("id", DataType::Utf8, false),
                Field::new("order_total", DataType::Float64, true),
            ]))),
            column_origins: vec![],
            findings: vec![],
        };

        let ddl = emit_ddl(&node).unwrap();
        assert!(ddl.contains("CREATE TABLE orders_clean"));
        assert!(ddl.contains("id VARCHAR NOT NULL"));
        assert!(ddl.contains("order_total DOUBLE"));
    }

    #[test]
    fn no_ddl_for_virtual_node() {
        let node = LineageNode {
            name: "prepared".to_string(),
            is_primary: false,
            input: Some("base".to_string()),
            clean_table: None,
            quarantine_table: None,
            is_virtual: true,
            banner: Banner::Virtual,
            output_schema: Some(Arc::new(Schema::empty())),
            column_origins: vec![],
            findings: vec![],
        };

        assert!(emit_ddl(&node).is_none());
    }
}
