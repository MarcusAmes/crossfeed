use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::schema::SchemaCatalog;
use crate::timeline::{TimelineInsertResult, TimelineRequest, TimelineResponse, TimelineStore};

#[derive(Debug)]
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|err| err.to_string())?;
        let store = Self { conn };
        store.initialize()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|err| err.to_string())?;
        let store = Self { conn };
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
                self.conn.execute(&index, []).map_err(|err| err.to_string())?;
            }
        }
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
