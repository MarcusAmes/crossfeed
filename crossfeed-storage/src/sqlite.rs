use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::query::{TimelineQuery, TimelineSort};
use crate::replay::{ReplayExecution, ReplayRequest, ReplayVersion};
use crate::schema::SchemaCatalog;
use crate::timeline::{TimelineInsertResult, TimelineRequest, TimelineResponse, TimelineStore};

#[derive(Debug, Clone)]
pub struct FtsConfig {
    pub enabled: bool,
    pub index_headers: bool,
    pub index_request_body: bool,
    pub index_response_body: bool,
}

impl Default for FtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            index_headers: true,
            index_request_body: false,
            index_response_body: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SqliteConfig {
    pub fts: FtsConfig,
}

#[derive(Debug)]
pub struct SqliteStore {
    conn: Connection,
    config: SqliteConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRequestSummary {
    pub id: i64,
    pub source: String,
    pub method: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: Option<String>,
    pub url: String,
    pub http_version: String,
    pub request_headers: Vec<u8>,
    pub request_body: Vec<u8>,
    pub request_body_size: usize,
    pub request_body_truncated: bool,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub scope_status_at_capture: String,
    pub scope_status_current: Option<String>,
    pub scope_rules_version: i64,
    pub capture_filtered: bool,
    pub timeline_filtered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSummary {
    pub status_code: u16,
    pub reason: Option<String>,
    pub header_count: usize,
    pub body_size: usize,
    pub body_truncated: bool,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::open_with_config(path, SqliteConfig::default())
    }

    pub fn open_with_config(path: impl AsRef<Path>, config: SqliteConfig) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|err| err.to_string())?;
        let store = Self { conn, config };
        store.initialize()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        Self::open_in_memory_with_config(SqliteConfig::default())
    }

    pub fn open_in_memory_with_config(config: SqliteConfig) -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|err| err.to_string())?;
        let store = Self { conn, config };
        store.initialize()?;
        Ok(store)
    }

    fn initialize(&self) -> Result<(), String> {
        self.conn
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|err| err.to_string())?;
        self.conn
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(|err| err.to_string())?;

        let schema = SchemaCatalog::v1();
        for table in schema.tables {
            self.conn
                .execute(&table.create_sql, [])
                .map_err(|err| err.to_string())?;
            for index in table.indices {
                let index_sql = index.replace("CREATE INDEX", "CREATE INDEX IF NOT EXISTS");
                self.conn
                    .execute(&index_sql, [])
                    .map_err(|err| err.to_string())?;
            }
        }

        if self.config.fts.enabled {
            self.create_fts_tables()?;
        }

        Ok(())
    }

    fn create_fts_tables(&self) -> Result<(), String> {
        self.conn
            .execute(
                "CREATE VIRTUAL TABLE IF NOT EXISTS timeline_requests_fts USING fts5(\
                    url, host, path, query, request_headers, request_body, response_headers, response_body\
                )",
                [],
            )
            .map_err(|err| err.to_string())?;

        self.conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS timeline_requests_fts_insert AFTER INSERT ON timeline_requests BEGIN\n                    INSERT INTO timeline_requests_fts (rowid, url, host, path, query, request_headers, request_body, response_headers, response_body)\n                    VALUES (new.id, new.url, new.host, new.path, new.query,\n                            CASE WHEN NEW.request_headers IS NOT NULL THEN CAST(NEW.request_headers AS TEXT) ELSE '' END,\n                            CASE WHEN NEW.request_body IS NOT NULL THEN CAST(NEW.request_body AS TEXT) ELSE '' END,\n                            '', '');\n                END;",
                [],
            )
            .map_err(|err| err.to_string())?;

        self.conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS timeline_requests_fts_delete AFTER DELETE ON timeline_requests BEGIN\n                    INSERT INTO timeline_requests_fts(timeline_requests_fts, rowid, url, host, path, query, request_headers, request_body, response_headers, response_body)\n                    VALUES('delete', old.id, old.url, old.host, old.path, old.query, '', '', '', '');\n                END;",
                [],
            )
            .map_err(|err| err.to_string())?;

        self.conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS timeline_requests_fts_update AFTER UPDATE ON timeline_requests BEGIN\n                    INSERT INTO timeline_requests_fts(timeline_requests_fts, rowid, url, host, path, query, request_headers, request_body, response_headers, response_body)\n                    VALUES('delete', old.id, old.url, old.host, old.path, old.query, '', '', '', '');\n                    INSERT INTO timeline_requests_fts (rowid, url, host, path, query, request_headers, request_body, response_headers, response_body)\n                    VALUES (new.id, new.url, new.host, new.path, new.query,\n                            CASE WHEN NEW.request_headers IS NOT NULL THEN CAST(NEW.request_headers AS TEXT) ELSE '' END,\n                            CASE WHEN NEW.request_body IS NOT NULL THEN CAST(NEW.request_body AS TEXT) ELSE '' END,\n                            '', '');\n                END;",
                [],
            )
            .map_err(|err| err.to_string())?;

        self.conn
            .execute(
                "CREATE TRIGGER IF NOT EXISTS timeline_responses_fts_update AFTER INSERT ON timeline_responses BEGIN\n                    UPDATE timeline_requests_fts\n                    SET response_headers = CASE WHEN NEW.response_headers IS NOT NULL THEN CAST(NEW.response_headers AS TEXT) ELSE '' END,\n                        response_body = CASE WHEN NEW.response_body IS NOT NULL THEN CAST(NEW.response_body AS TEXT) ELSE '' END\n                    WHERE rowid = NEW.timeline_request_id;\n                END;",
                [],
            )
            .map_err(|err| err.to_string())?;

        Ok(())
    }

    fn ensure_source_id(&self, source: &str) -> Result<i64, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM timeline_sources WHERE name = ?1")
            .map_err(|err| err.to_string())?;
        let existing = stmt
            .query_row([source], |row| row.get::<_, i64>(0))
            .optional()
            .map_err(|err| err.to_string())?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn
            .execute("INSERT INTO timeline_sources (name) VALUES (?1)", [source])
            .map_err(|err| err.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    fn insert_request_inner(&self, request: &TimelineRequest) -> Result<i64, String> {
        let source_id = self.ensure_source_id(&request.source)?;
        self.conn
            .execute(
                "INSERT INTO timeline_requests (
                    source_id, method, scheme, host, port, path, query, url,
                    http_version, request_headers, request_body, request_body_size,
                    request_body_truncated, started_at, completed_at, duration_ms,
                    scope_status_at_capture, scope_status_current, scope_rules_version,
                    capture_filtered, timeline_filtered
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                params![
                    source_id,
                    request.method,
                    request.scheme,
                    request.host,
                    request.port,
                    request.path,
                    request.query,
                    request.url,
                    request.http_version,
                    request.request_headers,
                    request.request_body,
                    request.request_body_size as i64,
                    request.request_body_truncated as i32,
                    request.started_at,
                    request.completed_at,
                    request.duration_ms,
                    request.scope_status_at_capture,
                    request.scope_status_current,
                    request.scope_rules_version,
                    request.capture_filtered as i32,
                    request.timeline_filtered as i32,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    fn insert_response_inner(&self, response: &TimelineResponse) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO timeline_responses (
                    timeline_request_id, status_code, reason, response_headers,
                    response_body, response_body_size, response_body_truncated,
                    http_version, received_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    response.timeline_request_id,
                    response.status_code,
                    response.reason,
                    response.response_headers,
                    response.response_body,
                    response.response_body_size as i64,
                    response.response_body_truncated as i32,
                    response.http_version,
                    response.received_at,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn ensure_tag_id(&self, name: &str) -> Result<i64, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM tags WHERE name = ?1")
            .map_err(|err| err.to_string())?;
        let existing = stmt
            .query_row([name], |row| row.get::<_, i64>(0))
            .optional()
            .map_err(|err| err.to_string())?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn
            .execute("INSERT INTO tags (name) VALUES (?1)", [name])
            .map_err(|err| err.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    fn insert_replay_request_inner(&self, request: &ReplayRequest) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO replay_requests (
                    collection_id, source_timeline_request_id, name, method, scheme, host, port,
                    path, query, url, http_version, request_headers, request_body, request_body_size,
                    active_version_id, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    request.collection_id,
                    request.source_timeline_request_id,
                    request.name,
                    request.method,
                    request.scheme,
                    request.host,
                    request.port as i64,
                    request.path,
                    request.query,
                    request.url,
                    request.http_version,
                    request.request_headers,
                    request.request_body,
                    request.request_body_size as i64,
                    request.active_version_id,
                    request.created_at,
                    request.updated_at,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    fn insert_replay_version_inner(&self, version: &ReplayVersion) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO replay_versions (
                    replay_request_id, parent_id, label, created_at, method, scheme, host, port,
                    path, query, url, http_version, request_headers, request_body, request_body_size
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    version.replay_request_id,
                    version.parent_id,
                    version.label,
                    version.created_at,
                    version.method,
                    version.scheme,
                    version.host,
                    version.port as i64,
                    version.path,
                    version.query,
                    version.url,
                    version.http_version,
                    version.request_headers,
                    version.request_body,
                    version.request_body_size as i64,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    fn update_replay_request_active_version(
        &self,
        request_id: i64,
        version_id: i64,
        updated_at: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE replay_requests SET active_version_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![version_id, updated_at, request_id],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn upsert_replay_request_snapshot(
        &self,
        request_id: i64,
        version: &ReplayVersion,
        updated_at: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE replay_requests SET
                    method = ?1,
                    scheme = ?2,
                    host = ?3,
                    port = ?4,
                    path = ?5,
                    query = ?6,
                    url = ?7,
                    http_version = ?8,
                    request_headers = ?9,
                    request_body = ?10,
                    request_body_size = ?11,
                    updated_at = ?12
                 WHERE id = ?13",
                params![
                    version.method,
                    version.scheme,
                    version.host,
                    version.port as i64,
                    version.path,
                    version.query,
                    version.url,
                    version.http_version,
                    version.request_headers,
                    version.request_body,
                    version.request_body_size as i64,
                    updated_at,
                    request_id,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn insert_replay_execution_inner(&self, execution: &ReplayExecution) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO replay_executions (replay_request_id, timeline_request_id, executed_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    execution.replay_request_id,
                    execution.timeline_request_id,
                    execution.executed_at,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }
}

impl TimelineStore for SqliteStore {
    fn insert_request(&self, request: TimelineRequest) -> Result<TimelineInsertResult, String> {
        let id = self.insert_request_inner(&request)?;
        Ok(TimelineInsertResult { request_id: id })
    }

    fn insert_response(&self, response: TimelineResponse) -> Result<(), String> {
        self.insert_response_inner(&response)
    }
}

impl SqliteStore {
    pub fn add_tags(&self, request_id: i64, tags: &[&str]) -> Result<(), String> {
        for tag in tags {
            let tag_id = self.ensure_tag_id(tag)?;
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO timeline_request_tags (timeline_request_id, tag_id) VALUES (?1, ?2)",
                    params![request_id, tag_id],
                )
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    pub fn get_request_tags(
        &self,
        request_ids: &[i64],
    ) -> Result<HashMap<i64, Vec<String>>, String> {
        if request_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = vec!["?"; request_ids.len()].join(", ");
        let sql = format!(
            "SELECT req_tag.timeline_request_id, tag.name FROM timeline_request_tags req_tag \
             JOIN tags tag ON tag.id = req_tag.tag_id \
             WHERE req_tag.timeline_request_id IN ({placeholders})"
        );
        let mut statement = self.conn.prepare(&sql).map_err(|err| err.to_string())?;
        let params = rusqlite::params_from_iter(request_ids.iter());
        let mut rows = statement.query(params).map_err(|err| err.to_string())?;
        let mut results: HashMap<i64, Vec<String>> = HashMap::new();
        while let Some(row) = rows.next().map_err(|err| err.to_string())? {
            let request_id: i64 = row.get(0).map_err(|err| err.to_string())?;
            let tag: String = row.get(1).map_err(|err| err.to_string())?;
            results.entry(request_id).or_default().push(tag);
        }
        Ok(results)
    }

    pub fn get_response_summaries(
        &self,
        request_ids: &[i64],
    ) -> Result<HashMap<i64, ResponseSummary>, String> {
        if request_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = vec!["?"; request_ids.len()].join(", ");
        let sql = format!(
            "SELECT timeline_request_id, status_code, reason, response_headers, response_body_size, response_body_truncated \
             FROM timeline_responses WHERE timeline_request_id IN ({placeholders})"
        );
        let mut statement = self.conn.prepare(&sql).map_err(|err| err.to_string())?;
        let params = rusqlite::params_from_iter(request_ids.iter());
        let mut rows = statement.query(params).map_err(|err| err.to_string())?;
        let mut results = HashMap::new();
        while let Some(row) = rows.next().map_err(|err| err.to_string())? {
            let request_id: i64 = row.get(0).map_err(|err| err.to_string())?;
            let headers: Vec<u8> = row.get(3).map_err(|err| err.to_string())?;
            let summary = ResponseSummary {
                status_code: row.get::<_, i64>(1).map_err(|err| err.to_string())? as u16,
                reason: row.get(2).map_err(|err| err.to_string())?,
                header_count: count_headers(&headers),
                body_size: row.get::<_, i64>(4).map_err(|err| err.to_string())? as usize,
                body_truncated: row.get::<_, i64>(5).map_err(|err| err.to_string())? != 0,
            };
            results.insert(request_id, summary);
        }
        Ok(results)
    }

    pub fn create_replay_request(&self, request: &ReplayRequest) -> Result<i64, String> {
        self.insert_replay_request_inner(request)
    }

    pub fn insert_replay_version(&self, version: &ReplayVersion) -> Result<i64, String> {
        self.insert_replay_version_inner(version)
    }

    pub fn update_replay_active_version(
        &self,
        request_id: i64,
        version_id: i64,
        updated_at: &str,
    ) -> Result<(), String> {
        self.update_replay_request_active_version(request_id, version_id, updated_at)
    }

    pub fn update_replay_snapshot(
        &self,
        request_id: i64,
        version: &ReplayVersion,
        updated_at: &str,
    ) -> Result<(), String> {
        self.upsert_replay_request_snapshot(request_id, version, updated_at)
    }

    pub fn insert_replay_execution(&self, execution: &ReplayExecution) -> Result<i64, String> {
        self.insert_replay_execution_inner(execution)
    }

    pub fn query_request_summaries(
        &self,
        query: &TimelineQuery,
        sort: TimelineSort,
    ) -> Result<Vec<TimelineRequestSummary>, String> {
        let mut sql = String::from(
            "SELECT DISTINCT req.id, source.name, req.method, req.scheme, req.host, req.port, req.path, req.query, req.url, req.http_version, req.request_headers, req.request_body, req.request_body_size, req.request_body_truncated, req.started_at, req.completed_at, req.duration_ms, req.scope_status_at_capture, req.scope_status_current, req.scope_rules_version, req.capture_filtered, req.timeline_filtered FROM timeline_requests req JOIN timeline_sources source ON req.source_id = source.id",
        );
        let mut where_clauses = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        let mut join_responses = false;
        let mut join_tags = false;

        if let Some(host) = &query.host {
            where_clauses.push("req.host = ?".to_string());
            params.push(host.clone().into());
        }
        if let Some(method) = &query.method {
            where_clauses.push("req.method = ?".to_string());
            params.push(method.clone().into());
        }
        if let Some(status) = &query.status {
            where_clauses.push("resp.status_code = ?".to_string());
            params.push((*status as i64).into());
            join_responses = true;
        }
        if let Some(scope_status) = &query.scope_status {
            where_clauses.push("req.scope_status_at_capture = ?".to_string());
            params.push(scope_status.clone().into());
        }
        if let Some(source) = &query.source {
            where_clauses.push("source.name = ?".to_string());
            params.push(source.clone().into());
        }
        if let Some(path_exact) = &query.path_exact {
            where_clauses.push("req.path = ?".to_string());
            params.push(path_exact.clone().into());
        }
        if let Some(path_prefix) = &query.path_prefix {
            if query.path_case_sensitive {
                where_clauses.push("req.path LIKE ?".to_string());
                params.push(format!("{path_prefix}%").into());
            } else {
                where_clauses.push("LOWER(req.path) LIKE LOWER(?)".to_string());
                params.push(format!("{path_prefix}%").into());
            }
        }
        if let Some(path_contains) = &query.path_contains {
            if query.path_case_sensitive {
                where_clauses.push("req.path LIKE ?".to_string());
                params.push(format!("%{path_contains}%").into());
            } else {
                where_clauses.push("LOWER(req.path) LIKE LOWER(?)".to_string());
                params.push(format!("%{path_contains}%").into());
            }
        }
        if !query.tags_any.is_empty() {
            join_tags = true;
            let placeholders = vec!["?"; query.tags_any.len()].join(", ");
            where_clauses.push(format!("tag.name IN ({placeholders})"));
            for tag in &query.tags_any {
                params.push(tag.clone().into());
            }
        }
        if let Some(since) = &query.since {
            where_clauses.push("req.started_at >= ?".to_string());
            params.push(since.clone().into());
        }
        if let Some(until) = &query.until {
            where_clauses.push("req.started_at <= ?".to_string());
            params.push(until.clone().into());
        }

        if let Some(search) = &query.search {
            sql.push_str(" JOIN timeline_requests_fts fts ON fts.rowid = req.id");
            where_clauses.push("timeline_requests_fts MATCH ?".to_string());
            params.push(search.clone().into());
        }

        if join_responses {
            sql.push_str(" LEFT JOIN timeline_responses resp ON resp.timeline_request_id = req.id");
        }
        if join_tags {
            sql.push_str(
                " JOIN timeline_request_tags req_tag ON req_tag.timeline_request_id = req.id",
            );
            sql.push_str(" JOIN tags tag ON tag.id = req_tag.tag_id");
        }

        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }

        match sort {
            TimelineSort::StartedAtDesc => sql.push_str(" ORDER BY req.started_at DESC"),
            TimelineSort::StartedAtAsc => sql.push_str(" ORDER BY req.started_at ASC"),
        }
        sql.push_str(" LIMIT ? OFFSET ?");
        params.push((query.limit as i64).into());
        params.push((query.offset as i64).into());

        let mut statement = self.conn.prepare(&sql).map_err(|err| err.to_string())?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                parse_request_summary_row(row)
            })
            .map_err(|err| err.to_string())?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|err| err.to_string())?);
        }
        Ok(results)
    }

    pub fn query_requests(
        &self,
        query: &TimelineQuery,
        sort: TimelineSort,
    ) -> Result<Vec<TimelineRequest>, String> {
        let summaries = self.query_request_summaries(query, sort)?;
        Ok(summaries.into_iter().map(TimelineRequest::from).collect())
    }

    pub fn get_request_summary(
        &self,
        request_id: i64,
    ) -> Result<Option<TimelineRequestSummary>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT req.id, source.name, req.method, req.scheme, req.host, req.port, req.path, req.query, req.url, req.http_version, req.request_headers, req.request_body, req.request_body_size, req.request_body_truncated, req.started_at, req.completed_at, req.duration_ms, req.scope_status_at_capture, req.scope_status_current, req.scope_rules_version, req.capture_filtered, req.timeline_filtered FROM timeline_requests req JOIN timeline_sources source ON req.source_id = source.id WHERE req.id = ?1",
            )
            .map_err(|err| err.to_string())?;
        statement
            .query_row([request_id], parse_request_summary_row)
            .optional()
            .map_err(|err| err.to_string())
    }

    pub fn get_response_by_request_id(
        &self,
        request_id: i64,
    ) -> Result<Option<TimelineResponse>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT timeline_request_id, status_code, reason, response_headers, response_body, response_body_size, response_body_truncated, http_version, received_at FROM timeline_responses WHERE timeline_request_id = ?1",
            )
            .map_err(|err| err.to_string())?;
        statement
            .query_row([request_id], parse_response_row)
            .optional()
            .map_err(|err| err.to_string())
    }
}

#[allow(dead_code)]
impl From<TimelineRequestSummary> for TimelineRequest {
    fn from(summary: TimelineRequestSummary) -> Self {
        Self {
            source: summary.source,
            method: summary.method,
            scheme: summary.scheme,
            host: summary.host,
            port: summary.port,
            path: summary.path,
            query: summary.query,
            url: summary.url,
            http_version: summary.http_version,
            request_headers: summary.request_headers,
            request_body: summary.request_body,
            request_body_size: summary.request_body_size,
            request_body_truncated: summary.request_body_truncated,
            started_at: summary.started_at,
            completed_at: summary.completed_at,
            duration_ms: summary.duration_ms,
            scope_status_at_capture: summary.scope_status_at_capture,
            scope_status_current: summary.scope_status_current,
            scope_rules_version: summary.scope_rules_version,
            capture_filtered: summary.capture_filtered,
            timeline_filtered: summary.timeline_filtered,
        }
    }
}

pub fn parse_request_summary_row(row: &Row<'_>) -> Result<TimelineRequestSummary, rusqlite::Error> {
    Ok(TimelineRequestSummary {
        id: row.get(0)?,
        source: row.get(1)?,
        method: row.get(2)?,
        scheme: row.get(3)?,
        host: row.get(4)?,
        port: row.get::<_, i64>(5)? as u16,
        path: row.get(6)?,
        query: row.get(7)?,
        url: row.get(8)?,
        http_version: row.get(9)?,
        request_headers: row.get(10)?,
        request_body: row.get(11)?,
        request_body_size: row.get::<_, i64>(12)? as usize,
        request_body_truncated: row.get::<_, i64>(13)? != 0,
        started_at: row.get(14)?,
        completed_at: row.get(15)?,
        duration_ms: row.get(16)?,
        scope_status_at_capture: row.get(17)?,
        scope_status_current: row.get(18)?,
        scope_rules_version: row.get(19)?,
        capture_filtered: row.get::<_, i64>(20)? != 0,
        timeline_filtered: row.get::<_, i64>(21)? != 0,
    })
}

pub fn parse_response_row(row: &Row<'_>) -> Result<TimelineResponse, rusqlite::Error> {
    Ok(TimelineResponse {
        timeline_request_id: row.get(0)?,
        status_code: row.get::<_, i64>(1)? as u16,
        reason: row.get(2)?,
        response_headers: row.get(3)?,
        response_body: row.get(4)?,
        response_body_size: row.get::<_, i64>(5)? as usize,
        response_body_truncated: row.get::<_, i64>(6)? != 0,
        http_version: row.get(7)?,
        received_at: row.get(8)?,
    })
}

fn count_headers(headers: &[u8]) -> usize {
    if headers.is_empty() {
        return 0;
    }
    let text = String::from_utf8_lossy(headers);
    text.lines().filter(|line| !line.trim().is_empty()).count()
}
