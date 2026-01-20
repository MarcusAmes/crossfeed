use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableSpec {
    pub name: String,
    pub create_sql: String,
    pub indices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaSpec {
    pub version: u32,
    pub tables: Vec<TableSpec>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchemaError {
    #[error("schema version must be greater than zero")]
    InvalidVersion,
    #[error("schema must include at least one table")]
    EmptyTables,
    #[error("table name cannot be empty")]
    EmptyTableName,
    #[error("table definition cannot be empty for {0}")]
    EmptyTableDefinition(String),
}

pub struct SchemaCatalog;

impl SchemaCatalog {
    pub fn v1() -> SchemaSpec {
        SchemaSpec {
            version: 1,
            tables: vec![
                TableSpec {
                    name: "timeline_sources".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS timeline_sources (\
    id INTEGER PRIMARY KEY,\
    name TEXT NOT NULL UNIQUE\
)"
                    .to_string(),
                    indices: vec![],
                },
                TableSpec {
                    name: "timeline_requests".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS timeline_requests (\
    id INTEGER PRIMARY KEY,\
    source_id INTEGER NOT NULL REFERENCES timeline_sources(id),\
    method TEXT NOT NULL,\
    scheme TEXT NOT NULL,\
    host TEXT NOT NULL,\
    port INTEGER NOT NULL,\
    path TEXT NOT NULL,\
    query TEXT,\
    url TEXT NOT NULL,\
    http_version TEXT NOT NULL,\
    request_headers BLOB NOT NULL,\
    request_body BLOB,\
    request_body_size INTEGER NOT NULL DEFAULT 0,\
    request_body_truncated INTEGER NOT NULL DEFAULT 0,\
    started_at TEXT NOT NULL,\
    completed_at TEXT,\
    duration_ms INTEGER,\
    scope_status_at_capture TEXT NOT NULL,\
    scope_status_current TEXT,\
    scope_rules_version INTEGER NOT NULL DEFAULT 1,\
    capture_filtered INTEGER NOT NULL DEFAULT 0,\
    timeline_filtered INTEGER NOT NULL DEFAULT 0\
)"
                    .to_string(),
                    indices: vec![
                        "CREATE INDEX idx_timeline_requests_started_at ON timeline_requests(started_at)"
                            .to_string(),
                        "CREATE INDEX idx_timeline_requests_host ON timeline_requests(host)".to_string(),
                        "CREATE INDEX idx_timeline_requests_source_id ON timeline_requests(source_id)"
                            .to_string(),
                        "CREATE INDEX idx_timeline_requests_method ON timeline_requests(method)"
                            .to_string(),
                        "CREATE INDEX idx_timeline_requests_scope_capture ON timeline_requests(scope_status_at_capture)"
                            .to_string(),
                        "CREATE INDEX idx_timeline_requests_scope_current ON timeline_requests(scope_status_current)"
                            .to_string(),
                        "CREATE INDEX idx_timeline_requests_url ON timeline_requests(url)".to_string(),
                        "CREATE INDEX idx_timeline_requests_path ON timeline_requests(path)".to_string(),
                    ],
                },
                TableSpec {
                    name: "timeline_responses".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS timeline_responses (\
    id INTEGER PRIMARY KEY,\
    timeline_request_id INTEGER NOT NULL REFERENCES timeline_requests(id),\
    status_code INTEGER NOT NULL,\
    reason TEXT,\
    response_headers BLOB NOT NULL,\
    response_body BLOB,\
    response_body_size INTEGER NOT NULL DEFAULT 0,\
    response_body_truncated INTEGER NOT NULL DEFAULT 0,\
    http_version TEXT NOT NULL,\
    received_at TEXT NOT NULL\
)"
                    .to_string(),
                    indices: vec![
                        "CREATE INDEX idx_timeline_responses_request_id ON timeline_responses(timeline_request_id)"
                            .to_string(),
                        "CREATE INDEX idx_timeline_responses_status_code ON timeline_responses(status_code)"
                            .to_string(),
                    ],
                },
                TableSpec {
                    name: "replay_collections".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS replay_collections (\
    id INTEGER PRIMARY KEY,\
    name TEXT NOT NULL,\
    created_at TEXT NOT NULL\
)"
                    .to_string(),
                    indices: vec![],
                },
                TableSpec {
                    name: "replay_requests".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS replay_requests (\
    id INTEGER PRIMARY KEY,\
    collection_id INTEGER REFERENCES replay_collections(id),\
    source_timeline_request_id INTEGER REFERENCES timeline_requests(id),\
    name TEXT NOT NULL,\
    method TEXT NOT NULL,\
    scheme TEXT NOT NULL,\
    host TEXT NOT NULL,\
    port INTEGER NOT NULL,\
    path TEXT NOT NULL,\
    query TEXT,\
    url TEXT NOT NULL,\
    http_version TEXT NOT NULL,\
    request_headers BLOB NOT NULL,\
    request_body BLOB,\
    request_body_size INTEGER NOT NULL DEFAULT 0,\
    active_version_id INTEGER REFERENCES replay_versions(id),\
    created_at TEXT NOT NULL,\
    updated_at TEXT NOT NULL\
 )"
                    .to_string(),
                    indices: vec![
                        "CREATE INDEX idx_replay_requests_collection_id ON replay_requests(collection_id)"
                            .to_string(),
                        "CREATE INDEX idx_replay_requests_source_timeline_request_id ON replay_requests(source_timeline_request_id)"
                            .to_string(),
                        "CREATE INDEX idx_replay_requests_active_version_id ON replay_requests(active_version_id)"
                            .to_string(),
                    ],
                },
                TableSpec {
                    name: "replay_versions".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS replay_versions (\
    id INTEGER PRIMARY KEY,\
    replay_request_id INTEGER NOT NULL REFERENCES replay_requests(id),\
    parent_id INTEGER REFERENCES replay_versions(id),\
    label TEXT,\
    created_at TEXT NOT NULL,\
    method TEXT NOT NULL,\
    scheme TEXT NOT NULL,\
    host TEXT NOT NULL,\
    port INTEGER NOT NULL,\
    path TEXT NOT NULL,\
    query TEXT,\
    url TEXT NOT NULL,\
    http_version TEXT NOT NULL,\
    request_headers BLOB NOT NULL,\
    request_body BLOB,\
    request_body_size INTEGER NOT NULL DEFAULT 0\
)"
                    .to_string(),
                    indices: vec![
                        "CREATE INDEX idx_replay_versions_replay_request_id ON replay_versions(replay_request_id)"
                            .to_string(),
                        "CREATE INDEX idx_replay_versions_parent_id ON replay_versions(parent_id)"
                            .to_string(),
                    ],
                },
                TableSpec {
                    name: "replay_executions".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS replay_executions (\
    id INTEGER PRIMARY KEY,\
    replay_request_id INTEGER NOT NULL REFERENCES replay_requests(id),\
    timeline_request_id INTEGER NOT NULL REFERENCES timeline_requests(id),\
    executed_at TEXT NOT NULL\
)"
                    .to_string(),
                    indices: vec![
                        "CREATE INDEX idx_replay_executions_replay_request_id ON replay_executions(replay_request_id)"
                            .to_string(),
                        "CREATE INDEX idx_replay_executions_timeline_request_id ON replay_executions(timeline_request_id)"
                            .to_string(),
                    ],
                },
                TableSpec {
                    name: "tags".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS tags (\
    id INTEGER PRIMARY KEY,\
    name TEXT NOT NULL UNIQUE\
)"
                    .to_string(),
                    indices: vec![],
                },
                TableSpec {
                    name: "timeline_request_tags".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS timeline_request_tags (\
    timeline_request_id INTEGER NOT NULL REFERENCES timeline_requests(id),\
    tag_id INTEGER NOT NULL REFERENCES tags(id),\
    PRIMARY KEY (timeline_request_id, tag_id)\
)"
                    .to_string(),
                    indices: vec!["CREATE INDEX idx_timeline_request_tags_tag_id ON timeline_request_tags(tag_id)"
                        .to_string()],
                },
                TableSpec {
                    name: "scope_rules".to_string(),
                    create_sql: "CREATE TABLE IF NOT EXISTS scope_rules (\
    id INTEGER PRIMARY KEY,\
    rule_type TEXT NOT NULL,\
    pattern_type TEXT NOT NULL,\
    target TEXT NOT NULL,\
    pattern TEXT NOT NULL,\
    enabled INTEGER NOT NULL DEFAULT 1,\
    created_at TEXT NOT NULL\
)"
                    .to_string(),
                    indices: vec![
                        "CREATE INDEX idx_scope_rules_target ON scope_rules(target)".to_string(),
                        "CREATE INDEX idx_scope_rules_enabled ON scope_rules(enabled)".to_string(),
                    ],
                },
            ],
        }
    }

    pub fn validate(schema: &SchemaSpec) -> Result<(), SchemaError> {
        if schema.version == 0 {
            return Err(SchemaError::InvalidVersion);
        }
        if schema.tables.is_empty() {
            return Err(SchemaError::EmptyTables);
        }
        for table in &schema.tables {
            if table.name.trim().is_empty() {
                return Err(SchemaError::EmptyTableName);
            }
            if table.create_sql.trim().is_empty() {
                return Err(SchemaError::EmptyTableDefinition(table.name.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SchemaCatalog, SchemaError, SchemaSpec, TableSpec};

    #[test]
    fn v1_schema_validates() {
        let schema = SchemaCatalog::v1();
        assert_eq!(SchemaCatalog::validate(&schema), Ok(()));
    }

    #[test]
    fn v1_schema_includes_expected_tables() {
        let schema = SchemaCatalog::v1();
        let names: Vec<&str> = schema
            .tables
            .iter()
            .map(|table| table.name.as_str())
            .collect();

        for required in [
            "timeline_sources",
            "timeline_requests",
            "timeline_responses",
            "replay_collections",
            "replay_requests",
            "replay_versions",
            "replay_executions",
            "tags",
            "timeline_request_tags",
            "scope_rules",
        ] {
            assert!(names.contains(&required), "missing table {required}");
        }
    }

    #[test]
    fn v1_schema_includes_replay_version_tree_index() {
        let schema = SchemaCatalog::v1();
        let table = schema
            .tables
            .iter()
            .find(|table| table.name == "replay_versions")
            .expect("replay_versions table exists");

        assert!(
            table
                .indices
                .iter()
                .any(|index| index.contains("parent_id")),
            "replay_versions should index parent_id"
        );
    }

    #[test]
    fn rejects_zero_version() {
        let schema = SchemaSpec {
            version: 0,
            tables: vec![TableSpec {
                name: "timeline_requests".to_string(),
                create_sql: "CREATE TABLE timeline_requests (...)".to_string(),
                indices: vec![],
            }],
        };

        assert_eq!(
            SchemaCatalog::validate(&schema),
            Err(SchemaError::InvalidVersion)
        );
    }

    #[test]
    fn rejects_empty_table_name() {
        let schema = SchemaSpec {
            version: 1,
            tables: vec![TableSpec {
                name: " ".to_string(),
                create_sql: "CREATE TABLE timeline_requests (...)".to_string(),
                indices: vec![],
            }],
        };

        assert_eq!(
            SchemaCatalog::validate(&schema),
            Err(SchemaError::EmptyTableName)
        );
    }

    #[test]
    fn rejects_empty_table_definition() {
        let schema = SchemaSpec {
            version: 1,
            tables: vec![TableSpec {
                name: "timeline_requests".to_string(),
                create_sql: " ".to_string(),
                indices: vec![],
            }],
        };

        assert_eq!(
            SchemaCatalog::validate(&schema),
            Err(SchemaError::EmptyTableDefinition(
                "timeline_requests".to_string()
            ))
        );
    }

    #[test]
    fn rejects_empty_tables_collection() {
        let schema = SchemaSpec {
            version: 1,
            tables: vec![],
        };

        assert_eq!(
            SchemaCatalog::validate(&schema),
            Err(SchemaError::EmptyTables)
        );
    }
}
