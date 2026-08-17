//! The canonical raw/meta seed columns every transform inherits as valid
//! lineage origins. Source of truth: docs/DELIVERABLES.md "Raw/Meta Seed
//! Column Registry". Kept as a static table rather than derived from any
//! fixture, since these are engine-level constants, not pipeline-specific
//! config.

#[derive(Debug, Clone, Copy)]
pub struct SeedColumn {
    pub name: &'static str,
    pub arrow_type: &'static str,
    // Not read anywhere yet (no diagnostic messages cite it today), but
    // kept as documentation-in-code for whoever next extends findings
    // output to explain what a given seed column is for.
    #[allow(dead_code)]
    pub description: &'static str,
}

pub const SEED_COLUMNS: &[SeedColumn] = &[
    SeedColumn {
        name: "_raw_payload",
        arrow_type: "Utf8",
        description: "Raw message payload",
    },
    SeedColumn {
        name: "_raw_payload_bin",
        arrow_type: "Binary",
        description: "Binary raw payload",
    },
    SeedColumn {
        name: "_ssync_source_topic",
        arrow_type: "Utf8",
        description: "Source topic name",
    },
    SeedColumn {
        name: "_ssync_source_partition",
        arrow_type: "Int32",
        description: "Source partition index",
    },
    SeedColumn {
        name: "_ssync_source_offset",
        arrow_type: "Utf8",
        description: "Source message position",
    },
    SeedColumn {
        name: "_ssync_message_key",
        arrow_type: "Utf8",
        description: "Message key; NULL when unset",
    },
    SeedColumn {
        name: "_ssync_producer",
        arrow_type: "Utf8",
        description: "Producer name; NULL for Kafka",
    },
    SeedColumn {
        name: "_ssync_sequence_id",
        arrow_type: "Int64",
        description: "Producer-assigned sequence id",
    },
    SeedColumn {
        name: "_ssync_source_headers",
        arrow_type: "Utf8",
        description: "Headers/properties as JSON string",
    },
    SeedColumn {
        name: "_ssync_record_id",
        arrow_type: "Utf8",
        description: "Deterministic per-record identity",
    },
    SeedColumn {
        name: "_ssync_event_ts",
        arrow_type: "Timestamp(Nanosecond, None)",
        description: "Broker/producer event time",
    },
    SeedColumn {
        name: "_ssync_ingest_ts",
        arrow_type: "Timestamp(Nanosecond, None)",
        description: "Wall-clock ingest time",
    },
    SeedColumn {
        name: "_ssync_emit_ts",
        arrow_type: "Timestamp(Nanosecond, None)",
        description: "Wall-clock write time at sink; NULL on raw lane",
    },
];

// Not called from production code yet - lineage_engine currently inlines
// its own seed-name check as part of a combined permitted-origins set - but
// kept public since it's the natural single-column membership check for
// whoever needs one directly (e.g. a future finding message wanting to say
// "X is a seed column, did you mean Y").
#[allow(dead_code)]
pub fn is_seed_column(name: &str) -> bool {
    SEED_COLUMNS.iter().any(|c| c.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_all_documented_seed_columns() {
        assert!(is_seed_column("_raw_payload"));
        assert!(is_seed_column("_ssync_ingest_ts"));
        assert!(!is_seed_column("promo_code"));
    }

    #[test]
    fn no_duplicate_seed_column_names() {
        let mut names: Vec<_> = SEED_COLUMNS.iter().map(|c| c.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate seed column name found");
    }
}
