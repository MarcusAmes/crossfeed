use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::query::{TimelineQuery, TimelineSort};
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
    pub fn query_requests(
        &self,
        query: &TimelineQuery,
        sort: TimelineSort,
    ) -> Result<Vec<TimelineRequest>, String> {
        let mut sql = String::from(
            "SELECT source.name, req.method, req.scheme, req.host, req.port, req.path, req.query, req.url, req.http_version, req.request_headers, req.request_body, req.request_body_size, req.request_body_truncated, req.started_at, req.completed_at, req.duration_ms, req.scope_status_at_capture, req.scope_status_current, req.scope_rules_version, req.capture_filtered, req.timeline_filtered FROM timeline_requests req JOIN timeline_sources source ON req.source_id = source.id",
        );
        let mut where_clauses = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        let mut join_responses = false;

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
                parse_request_row(row)
            })
            .map_err(|err| err.to_string())?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|err| err.to_string())?);
        }
        Ok(results)
    }
}

#[allow(dead_code)]
pub fn parse_request_row(row: &Row<'_>) -> Result<TimelineRequest, rusqlite::Error> {
    Ok(TimelineRequest {
        source: row.get(0)?,
        method: row.get(1)?,
        scheme: row.get(2)?,
        host: row.get(3)?,
        port: row.get::<_, i64>(4)? as u16,
        path: row.get(5)?,
        query: row.get(6)?,
        url: row.get(7)?,
        http_version: row.get(8)?,
        request_headers: row.get(9)?,
        request_body: row.get(10)?,
        request_body_size: row.get::<_, i64>(11)? as usize,
        request_body_truncated: row.get::<_, i64>(12)? != 0,
        started_at: row.get(13)?,
        completed_at: row.get(14)?,
        duration_ms: row.get(15)?,
        scope_status_at_capture: row.get(16)?,
        scope_status_current: row.get(17)?,
        scope_rules_version: row.get(18)?,
        capture_filtered: row.get::<_, i64>(19)? != 0,
        timeline_filtered: row.get::<_, i64>(20)? != 0,
    })
}
