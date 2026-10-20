//! Persistencia local en SQLite. Las fechas se guardan como segundos Unix (UTC).

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Error;
use crate::model::{Occurrence, Reminder, ReminderSpec};

const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE reminders (
        id            INTEGER PRIMARY KEY,
        spec          TEXT    NOT NULL,
        next_fire_at  INTEGER,
        pending_since INTEGER,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL
    );
    CREATE TABLE occurrences (
        id          INTEGER PRIMARY KEY,
        reminder_id INTEGER NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
        due_at      INTEGER NOT NULL,
        action      TEXT    NOT NULL,
        acted_at    INTEGER NOT NULL
    );
    CREATE INDEX occurrences_acted_at ON occurrences(acted_at DESC);
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"#];

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, Error> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, Error> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", i as i64 + 1)?;
        }
        Ok(Self { conn })
    }

    pub fn insert(
        &self,
        spec: &ReminderSpec,
        next_fire_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<i64, Error> {
        self.conn.execute(
            "INSERT INTO reminders (spec, next_fire_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![serde_json::to_string(spec)?, ts(next_fire_at), now.timestamp()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Reemplaza la configuración y reinicia el estado de la alerta.
    pub fn update_spec(
        &self,
        id: i64,
        spec: &ReminderSpec,
        next_fire_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<(), Error> {
        let n = self.conn.execute(
            "UPDATE reminders SET spec = ?2, next_fire_at = ?3, pending_since = NULL, updated_at = ?4 WHERE id = ?1",
            params![id, serde_json::to_string(spec)?, ts(next_fire_at), now.timestamp()],
        )?;
        found(n, id)
    }

    pub fn set_state(
        &self,
        id: i64,
        next_fire_at: Option<DateTime<Utc>>,
        pending_since: Option<DateTime<Utc>>,
    ) -> Result<(), Error> {
        let n = self.conn.execute(
            "UPDATE reminders SET next_fire_at = ?2, pending_since = ?3 WHERE id = ?1",
            params![id, ts(next_fire_at), ts(pending_since)],
        )?;
        found(n, id)
    }

    pub fn delete(&self, id: i64) -> Result<(), Error> {
        let n = self.conn.execute("DELETE FROM reminders WHERE id = ?1", [id])?;
        found(n, id)
    }

    pub fn get(&self, id: i64) -> Result<Reminder, Error> {
        self.conn
            .query_row(&format!("{SELECT_REMINDER} WHERE id = ?1"), [id], reminder_row)
            .optional()?
            .ok_or(Error::NotFound(id))?
    }

    pub fn list(&self) -> Result<Vec<Reminder>, Error> {
        let mut stmt = self.conn.prepare(&format!("{SELECT_REMINDER} ORDER BY id"))?;
        let rows = stmt.query_map([], reminder_row)?;
        rows.map(|r| r?).collect()
    }

    pub fn add_occurrence(
        &self,
        reminder_id: i64,
        due_at: DateTime<Utc>,
        action: &str,
        acted_at: DateTime<Utc>,
    ) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO occurrences (reminder_id, due_at, action, acted_at) VALUES (?1, ?2, ?3, ?4)",
            params![reminder_id, due_at.timestamp(), action, acted_at.timestamp()],
        )?;
        Ok(())
    }

    pub fn history(&self, limit: u32) -> Result<Vec<Occurrence>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT o.id, o.reminder_id, r.spec, o.due_at, o.action, o.acted_at
             FROM occurrences o JOIN reminders r ON r.id = o.reminder_id
             ORDER BY o.acted_at DESC, o.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            let spec: String = row.get(2)?;
            Ok((row.get(0)?, row.get(1)?, spec, row.get(3)?, row.get(4)?, row.get(5)?))
        })?;
        rows.map(|r| {
            let (id, reminder_id, spec, due_at, action, acted_at) = r?;
            let spec: ReminderSpec = serde_json::from_str(&spec)?;
            Ok(Occurrence {
                id,
                reminder_id,
                title: spec.title,
                due_at: from_ts(due_at),
                action,
                acted_at: from_ts(acted_at),
            })
        })
        .collect()
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, Error> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: Option<&str>) -> Result<(), Error> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, v],
            )?,
            None => self.conn.execute("DELETE FROM settings WHERE key = ?1", [key])?,
        };
        Ok(())
    }
}

const SELECT_REMINDER: &str = "SELECT id, spec, next_fire_at, pending_since, created_at FROM reminders";

/// Lee una fila de `reminders`. El error externo es de SQLite y el interno de JSON.
fn reminder_row(row: &Row<'_>) -> rusqlite::Result<Result<Reminder, Error>> {
    let spec: String = row.get(1)?;
    let next_fire_at: Option<i64> = row.get(2)?;
    let pending_since: Option<i64> = row.get(3)?;
    let id = row.get(0)?;
    let created_at = row.get(4)?;
    Ok(serde_json::from_str(&spec).map_err(Error::from).map(|spec| Reminder {
        id,
        spec,
        next_fire_at: next_fire_at.map(from_ts),
        pending_since: pending_since.map(from_ts),
        created_at: from_ts(created_at),
    }))
}

fn found(rows: usize, id: i64) -> Result<(), Error> {
    if rows == 0 { Err(Error::NotFound(id)) } else { Ok(()) }
}

fn ts(t: Option<DateTime<Utc>>) -> Option<i64> {
    t.map(|t| t.timestamp())
}

fn from_ts(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap_or_default()
}
