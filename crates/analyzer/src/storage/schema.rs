use rusqlite::{Connection, Result};

pub fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;

        CREATE TABLE IF NOT EXISTS sources (
            id          INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            port        INTEGER NOT NULL UNIQUE CHECK (port BETWEEN 1 AND 65535),
            enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
            deleted     INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1))
        );

        CREATE TABLE IF NOT EXISTS segments (
            id          INTEGER PRIMARY KEY,
            filename    TEXT NOT NULL UNIQUE,
            created_at  INTEGER NOT NULL,
            closed_at   INTEGER,
            size_bytes  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id                  INTEGER PRIMARY KEY,
            segment_id          INTEGER NOT NULL REFERENCES segments(id),
            segment_offset      INTEGER NOT NULL,
            record_length       INTEGER NOT NULL,
            started_at          INTEGER NOT NULL,
            ended_at            INTEGER NOT NULL,
            source_id           INTEGER NOT NULL REFERENCES sources(id),
            client_ip           BLOB NOT NULL,
            client_port         INTEGER NOT NULL,
            server_ip           BLOB NOT NULL,
            server_port         INTEGER NOT NULL,
            protocol            INTEGER NOT NULL,
            bytes_c2s           INTEGER NOT NULL,
            bytes_s2c           INTEGER NOT NULL,
            contains_flag       INTEGER NOT NULL DEFAULT 0,
            flag_direction      INTEGER NOT NULL DEFAULT 0,
            flag_count          INTEGER NOT NULL DEFAULT 0,
            suricata_alerts     INTEGER NOT NULL DEFAULT 0,
            incomplete          INTEGER NOT NULL DEFAULT 0,
            http_method         TEXT,
            http_host           TEXT,
            http_path           TEXT,
            http_status         INTEGER,
            http_content_type   TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_started_at
            ON sessions(started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_sessions_source_time
            ON sessions(source_id, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_sessions_flag_time
            ON sessions(contains_flag, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_sessions_segment
            ON sessions(segment_id);

        DELETE FROM sessions
        WHERE protocol = 0 AND bytes_c2s = 0 AND bytes_s2c = 0;
        "#,
    )
}

#[cfg(test)]
mod tests {
    use super::migrate;

    #[test]
    fn migration_removes_legacy_empty_raw_tcp_sessions() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO sources(id, name, port) VALUES (1, 'web', 8080)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO segments(id, filename, created_at) VALUES (1, 'test.seg', 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sessions(
                id, segment_id, segment_offset, record_length, started_at, ended_at,
                source_id, client_ip, client_port, server_ip, server_port, protocol,
                bytes_c2s, bytes_s2c
             ) VALUES (1, 1, 0, 0, 1, 2, 1, X'0A000002', 50000, X'0A000001', 8080, 0, 0, 0)",
                [],
            )
            .unwrap();

        migrate(&connection).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
