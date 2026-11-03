//! Local SQLite database. Secrets are stored only as ciphertext.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use termix_core::{Machine, MachineId, NewMachine, Protocol};
use termix_security::Vault;
use uuid::Uuid;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS machines (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username TEXT NOT NULL,
    protocol TEXT NOT NULL,
    os_hint TEXT,
    favorite INTEGER NOT NULL DEFAULT 0,
    notes TEXT NOT NULL DEFAULT '',
    group_name TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_connected_at TEXT
);
CREATE TABLE IF NOT EXISTS machine_tags (
    machine_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (machine_id, tag)
);
CREATE TABLE IF NOT EXISTS known_hosts (
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    algorithm TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    trusted_at TEXT NOT NULL,
    PRIMARY KEY (host, port)
);
CREATE TABLE IF NOT EXISTS secrets (
    id TEXT PRIMARY KEY,
    machine_id TEXT,
    kind TEXT NOT NULL,
    ciphertext TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS command_templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    body TEXT NOT NULL,
    destructive INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS activity_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    machine_id TEXT,
    action TEXT NOT NULL,
    detail TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Core(#[from] termix_core::CoreError),
    #[error(transparent)]
    Crypto(#[from] termix_security::CryptoError),
    #[error("machine {0} was not found")]
    NotFound(Uuid),
    #[error("no machine named {0}")]
    UnknownName(String),
    #[error("multiple machines named {0}; use the id")]
    AmbiguousName(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (1)",
            [],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_machine(&self, machine: NewMachine) -> Result<Machine, StoreError> {
        machine.validate()?;
        let now = Utc::now();
        let id = Uuid::new_v4();
        let conn = self.conn.lock().expect("database lock");
        conn.execute(
            "INSERT INTO machines (id, name, host, port, username, protocol, os_hint, favorite, notes, group_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?10)",
            params![
                id.to_string(),
                machine.name.trim(),
                machine.host.trim(),
                machine.port,
                machine.username.trim(),
                machine.protocol.as_str(),
                machine.os_hint,
                machine.notes,
                machine.group,
                now.to_rfc3339(),
            ],
        )?;
        for tag in &machine.tags {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }
            conn.execute(
                "INSERT OR IGNORE INTO machine_tags (machine_id, tag) VALUES (?1, ?2)",
                params![id.to_string(), tag],
            )?;
        }
        drop(conn);
        self.machine(id)?.ok_or(StoreError::NotFound(id))
    }

    pub fn list_machines(&self) -> Result<Vec<Machine>, StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, username, protocol, os_hint, favorite, notes, group_name, created_at, updated_at, last_connected_at
             FROM machines ORDER BY favorite DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], map_machine)?;
        let mut machines = Vec::new();
        for row in rows {
            let mut machine = row?;
            machine.tags = tags_for(&conn, machine.id)?;
            machines.push(machine);
        }
        Ok(machines)
    }

    pub fn machine(&self, id: MachineId) -> Result<Option<Machine>, StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, username, protocol, os_hint, favorite, notes, group_name, created_at, updated_at, last_connected_at
             FROM machines WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            let mut machine = map_machine(row)?;
            machine.tags = tags_for(&conn, machine.id)?;
            Ok(Some(machine))
        } else {
            Ok(None)
        }
    }

    /// Accept a UUID or a unique saved name. Names are matched case-insensitively.
    pub fn resolve(&self, id_or_name: &str) -> Result<Machine, StoreError> {
        let key = id_or_name.trim();
        if key.is_empty() {
            return Err(StoreError::UnknownName(key.to_string()));
        }
        if let Ok(id) = Uuid::parse_str(key) {
            return self.machine(id)?.ok_or(StoreError::NotFound(id));
        }
        let conn = self.conn.lock().expect("database lock");
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, username, protocol, os_hint, favorite, notes, group_name, created_at, updated_at, last_connected_at
             FROM machines WHERE name = ?1 COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![key], map_machine)?;
        let mut matches = Vec::new();
        for row in rows {
            let mut machine = row?;
            machine.tags = tags_for(&conn, machine.id)?;
            matches.push(machine);
        }
        match matches.len() {
            0 => Err(StoreError::UnknownName(key.to_string())),
            1 => Ok(matches.remove(0)),
            _ => Err(StoreError::AmbiguousName(key.to_string())),
        }
    }

    pub fn set_favorite(&self, id: MachineId, favorite: bool) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let n = conn.execute(
            "UPDATE machines SET favorite = ?1, updated_at = ?2 WHERE id = ?3",
            params![favorite as i64, Utc::now().to_rfc3339(), id.to_string()],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    pub fn delete_machine(&self, id: MachineId) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("database lock");
        conn.execute(
            "DELETE FROM machine_tags WHERE machine_id = ?1",
            params![id.to_string()],
        )?;
        conn.execute(
            "DELETE FROM secrets WHERE machine_id = ?1",
            params![id.to_string()],
        )?;
        let n = conn.execute(
            "DELETE FROM machines WHERE id = ?1",
            params![id.to_string()],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    pub fn mark_connected(&self, id: MachineId) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE machines SET last_connected_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn trust_host(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("database lock");
        conn.execute(
            "INSERT INTO known_hosts (host, port, algorithm, fingerprint, trusted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(host, port) DO UPDATE SET algorithm = excluded.algorithm, fingerprint = excluded.fingerprint, trusted_at = excluded.trusted_at",
            params![host, port, algorithm, fingerprint, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn trusted_fingerprint(&self, host: &str, port: u16) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let mut stmt =
            conn.prepare("SELECT fingerprint FROM known_hosts WHERE host = ?1 AND port = ?2")?;
        let mut rows = stmt.query(params![host, port])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn put_secret(
        &self,
        vault: &Vault,
        machine_id: Option<MachineId>,
        kind: &str,
        plaintext: &[u8],
    ) -> Result<Uuid, StoreError> {
        let ciphertext = vault.encrypt(plaintext)?;
        let id = Uuid::new_v4();
        let conn = self.conn.lock().expect("database lock");
        conn.execute(
            "INSERT INTO secrets (id, machine_id, kind, ciphertext, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                machine_id.map(|id| id.to_string()),
                kind,
                ciphertext,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(id)
    }

    pub fn read_secret(&self, vault: &Vault, id: Uuid) -> Result<Vec<u8>, StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let ciphertext: String = conn.query_row(
            "SELECT ciphertext FROM secrets WHERE id = ?1",
            params![id.to_string()],
            |row| row.get(0),
        )?;
        Ok(vault.decrypt(&ciphertext)?)
    }

    pub fn insert_template(
        &self,
        name: &str,
        body: &str,
        destructive: bool,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let conn = self.conn.lock().expect("database lock");
        conn.execute(
            "INSERT INTO command_templates (id, name, body, destructive, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.to_string(), name, body, destructive as i64, Utc::now().to_rfc3339()],
        )?;
        Ok(id)
    }

    pub fn list_templates(&self) -> Result<Vec<Template>, StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let mut stmt = conn.prepare(
            "SELECT id, name, body, destructive FROM command_templates ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Template {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                name: row.get(1)?,
                body: row.get(2)?,
                destructive: row.get::<_, i64>(3)? != 0,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn log(
        &self,
        machine_id: Option<MachineId>,
        action: &str,
        detail: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("database lock");
        conn.execute(
            "INSERT INTO activity_log (at, machine_id, action, detail) VALUES (?1, ?2, ?3, ?4)",
            params![
                Utc::now().to_rfc3339(),
                machine_id.map(|id| id.to_string()),
                action,
                detail,
            ],
        )?;
        Ok(())
    }

    pub fn recent_activity(&self, limit: usize) -> Result<Vec<Activity>, StoreError> {
        let conn = self.conn.lock().expect("database lock");
        let mut stmt = conn.prepare(
            "SELECT at, machine_id, action, detail FROM activity_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let machine: Option<String> = row.get(1)?;
            Ok(Activity {
                at: row.get(0)?,
                machine_id: machine.and_then(|s| Uuid::parse_str(&s).ok()),
                action: row.get(2)?,
                detail: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub body: String,
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub at: String,
    pub machine_id: Option<MachineId>,
    pub action: String,
    pub detail: String,
}

fn tags_for(conn: &Connection, id: MachineId) -> rusqlite::Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT tag FROM machine_tags WHERE machine_id = ?1 ORDER BY tag")?;
    let rows = stmt.query_map(params![id.to_string()], |row| row.get(0))?;
    rows.collect()
}

fn map_machine(row: &rusqlite::Row<'_>) -> rusqlite::Result<Machine> {
    let protocol = Protocol::parse(&row.get::<_, String>(5)?).unwrap_or(Protocol::Ssh);
    Ok(Machine {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        protocol,
        os_hint: row.get(6)?,
        favorite: row.get::<_, i64>(7)? != 0,
        notes: row.get(8)?,
        group: row.get(9)?,
        created_at: parse_time(&row.get::<_, String>(10)?),
        updated_at: parse_time(&row.get::<_, String>(11)?),
        last_connected_at: row.get::<_, Option<String>>(12)?.map(|s| parse_time(&s)),
        tags: Vec::new(),
    })
}

fn parse_time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

pub fn default_db_path() -> PathBuf {
    data_dir().join("termix.sqlite")
}

pub fn default_key_path() -> PathBuf {
    data_dir().join("master.key")
}

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "Termix", "Termix")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".termix"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use termix_core::NewMachine;

    fn sample() -> NewMachine {
        NewMachine {
            name: "edge".into(),
            host: "10.0.0.8".into(),
            port: 22,
            username: "ops".into(),
            protocol: Protocol::Ssh,
            os_hint: Some("linux".into()),
            notes: String::new(),
            tags: vec!["prod".into(), "edge".into()],
            group: Some("production".into()),
        }
    }

    #[test]
    fn machines_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let created = db.insert_machine(sample()).unwrap();
        assert_eq!(created.tags, vec!["edge".to_string(), "prod".to_string()]);
        db.set_favorite(created.id, true).unwrap();
        let listed = db.list_machines().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].favorite);
        db.trust_host("10.0.0.8", 22, "ssh-ed25519", "SHA256:abc")
            .unwrap();
        assert_eq!(
            db.trusted_fingerprint("10.0.0.8", 22).unwrap().as_deref(),
            Some("SHA256:abc")
        );
        assert_eq!(db.resolve("EDGE").unwrap().id, created.id);
        assert_eq!(db.resolve(&created.id.to_string()).unwrap().id, created.id);
    }

    #[test]
    fn resolve_rejects_unknown_and_duplicate_names() {
        let db = Database::open_in_memory().unwrap();
        db.insert_machine(sample()).unwrap();
        let mut twin = sample();
        twin.host = "10.0.0.9".into();
        db.insert_machine(twin).unwrap();
        assert!(matches!(
            db.resolve("missing"),
            Err(StoreError::UnknownName(_))
        ));
        assert!(matches!(
            db.resolve("edge"),
            Err(StoreError::AmbiguousName(_))
        ));
    }

    #[test]
    fn secrets_are_not_plaintext() {
        let db = Database::open_in_memory().unwrap();
        let vault = Vault::generate();
        let id = db.put_secret(&vault, None, "password", b"hunter2").unwrap();
        let conn = db.conn.lock().unwrap();
        let stored: String = conn
            .query_row(
                "SELECT ciphertext FROM secrets WHERE id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert!(!stored.contains("hunter2"));
        assert_eq!(db.read_secret(&vault, id).unwrap(), b"hunter2");
    }
}
