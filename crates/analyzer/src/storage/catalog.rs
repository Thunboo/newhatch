use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};

use crate::domain::{
    FlagDirection, HttpMetadata, SessionProtocol, SessionSummary, Source, SourceInput,
    StoredSession,
};

use super::schema;

#[derive(Clone, Debug)]
pub struct Catalog {
    path: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct SessionFilter {
    pub source_id: Option<i64>,
    pub contains_flag: Option<bool>,
    pub protocol: Option<SessionProtocol>,
    pub client_ip: Option<IpAddr>,
    pub server_ip: Option<IpAddr>,
    pub client_port: Option<u16>,
    pub server_port: Option<u16>,
    pub started_after: Option<i64>,
    pub cursor: Option<i64>,
    pub limit: usize,
}

impl Catalog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create data directory {}", parent.display()))?;
        }
        let connection = open_connection(&path)?;
        schema::migrate(&connection).context("migrate SQLite catalog")?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> Result<Connection> {
        open_connection(&self.path)
    }

    pub fn list_sources(&self, enabled_only: bool) -> Result<Vec<Source>> {
        let connection = self.connection()?;
        let sql = if enabled_only {
            "SELECT id, name, port, enabled FROM sources WHERE enabled = 1 AND deleted = 0 ORDER BY port"
        } else {
            "SELECT id, name, port, enabled FROM sources WHERE deleted = 0 ORDER BY port"
        };
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map([], map_source)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("list sources")
    }

    pub fn create_source(&self, input: &SourceInput) -> Result<Source> {
        validate_source(input)?;
        let connection = self.connection()?;
        let restored = connection
            .execute(
                "UPDATE sources SET name = ?1, enabled = ?2, deleted = 0 WHERE port = ?3 AND deleted = 1",
                params![input.name.trim(), input.enabled, input.port],
            )
            .context("restore source")?;
        if restored > 0 {
            let id = connection.query_row(
                "SELECT id FROM sources WHERE port = ?1 AND deleted = 0",
                [input.port],
                |row| row.get(0),
            )?;
            return Ok(Source {
                id,
                name: input.name.trim().to_owned(),
                port: input.port,
                enabled: input.enabled,
            });
        }
        connection
            .execute(
                "INSERT INTO sources(name, port, enabled) VALUES (?1, ?2, ?3)",
                params![input.name.trim(), input.port, input.enabled],
            )
            .context("insert source")?;
        let id = connection.last_insert_rowid();
        Ok(Source {
            id,
            name: input.name.trim().to_owned(),
            port: input.port,
            enabled: input.enabled,
        })
    }

    pub fn update_source(&self, id: i64, input: &SourceInput) -> Result<Option<Source>> {
        validate_source(input)?;
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE sources SET name = ?1, port = ?2, enabled = ?3 WHERE id = ?4 AND deleted = 0",
                params![input.name.trim(), input.port, input.enabled, id],
            )
            .context("update source")?;
        if changed == 0 {
            return Ok(None);
        }
        Ok(Some(Source {
            id,
            name: input.name.trim().to_owned(),
            port: input.port,
            enabled: input.enabled,
        }))
    }

    pub fn delete_source(&self, id: i64) -> Result<bool> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE sources SET enabled = 0, deleted = 1 WHERE id = ?1 AND deleted = 0",
                [id],
            )
            .context("delete source")?;
        Ok(changed > 0)
    }

    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<SessionSummary>> {
        let mut sql = String::from(
            r#"
            SELECT s.id, s.source_id, src.name, s.started_at, s.ended_at,
                   s.client_ip, s.client_port, s.server_ip, s.server_port,
                   s.protocol, s.bytes_c2s, s.bytes_s2c, s.contains_flag,
                   s.flag_direction, s.flag_count, s.suricata_alerts, s.incomplete,
                   s.http_method, s.http_host, s.http_path, s.http_status,
                   s.http_content_type
            FROM sessions s
            JOIN sources src ON src.id = s.source_id
            WHERE 1 = 1
            "#,
        );
        let mut values = Vec::<Value>::new();

        push_condition(&mut sql, &mut values, "s.source_id = ?", filter.source_id);
        push_condition(
            &mut sql,
            &mut values,
            "s.contains_flag = ?",
            filter.contains_flag.map(i64::from),
        );
        push_condition(
            &mut sql,
            &mut values,
            "s.protocol = ?",
            filter.protocol.map(|value| value as i64),
        );
        push_condition(
            &mut sql,
            &mut values,
            "s.client_ip = ?",
            filter.client_ip.map(|value| Value::Blob(ip_to_blob(value))),
        );
        push_condition(
            &mut sql,
            &mut values,
            "s.server_ip = ?",
            filter.server_ip.map(|value| Value::Blob(ip_to_blob(value))),
        );
        push_condition(
            &mut sql,
            &mut values,
            "s.client_port = ?",
            filter.client_port.map(i64::from),
        );
        push_condition(
            &mut sql,
            &mut values,
            "s.server_port = ?",
            filter.server_port.map(i64::from),
        );
        push_condition(
            &mut sql,
            &mut values,
            "s.started_at >= ?",
            filter.started_after,
        );
        push_condition(&mut sql, &mut values, "s.id < ?", filter.cursor);

        sql.push_str(" ORDER BY s.id DESC LIMIT ?");
        values.push(Value::Integer(filter.limit.clamp(1, 200) as i64));

        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), map_session_summary)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("list sessions")
    }

    pub fn get_session(&self, id: i64) -> Result<Option<StoredSession>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT s.id, s.source_id, src.name, s.started_at, s.ended_at,
                       s.client_ip, s.client_port, s.server_ip, s.server_port,
                       s.protocol, s.bytes_c2s, s.bytes_s2c, s.contains_flag,
                       s.flag_direction, s.flag_count, s.suricata_alerts, s.incomplete,
                       s.http_method, s.http_host, s.http_path, s.http_status,
                       s.http_content_type, s.segment_id, seg.filename,
                       s.segment_offset, s.record_length
                FROM sessions s
                JOIN sources src ON src.id = s.source_id
                JOIN segments seg ON seg.id = s.segment_id
                WHERE s.id = ?1
                "#,
                [id],
                |row| {
                    Ok(StoredSession {
                        summary: map_session_summary(row)?,
                        segment_id: row.get(22)?,
                        segment_filename: row.get(23)?,
                        segment_offset: row.get::<_, i64>(24)? as u64,
                        record_length: row.get::<_, i64>(25)? as u64,
                    })
                },
            )
            .optional()
            .context("get session")
    }
}

fn open_connection(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)
        .with_context(|| format!("open SQLite catalog {}", path.display()))?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(connection)
}

fn validate_source(input: &SourceInput) -> Result<()> {
    if input.name.trim().is_empty() {
        bail!("source name must not be empty");
    }
    if input.name.trim().len() > 80 {
        bail!("source name must not exceed 80 bytes");
    }
    if input.port == 0 {
        bail!("source port must be between 1 and 65535");
    }
    Ok(())
}

fn map_source(row: &Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: row.get(0)?,
        name: row.get(1)?,
        port: row.get::<_, i64>(2)? as u16,
        enabled: row.get(3)?,
    })
}

fn map_session_summary(row: &Row<'_>) -> rusqlite::Result<SessionSummary> {
    let protocol_value: i64 = row.get(9)?;
    let direction_value: i64 = row.get(13)?;
    Ok(SessionSummary {
        id: row.get(0)?,
        source_id: row.get(1)?,
        source_name: row.get(2)?,
        started_at: row.get(3)?,
        ended_at: row.get(4)?,
        client_ip: blob_to_ip(&row.get::<_, Vec<u8>>(5)?).map_err(to_sql_error)?,
        client_port: row.get::<_, i64>(6)? as u16,
        server_ip: blob_to_ip(&row.get::<_, Vec<u8>>(7)?).map_err(to_sql_error)?,
        server_port: row.get::<_, i64>(8)? as u16,
        protocol: SessionProtocol::from_db(protocol_value)
            .ok_or_else(|| to_sql_error(anyhow!("unknown protocol value {protocol_value}")))?,
        bytes_c2s: row.get::<_, i64>(10)? as u64,
        bytes_s2c: row.get::<_, i64>(11)? as u64,
        contains_flag: row.get(12)?,
        flag_direction: FlagDirection::from_db(direction_value).ok_or_else(|| {
            to_sql_error(anyhow!("unknown flag direction value {direction_value}"))
        })?,
        flag_count: row.get::<_, i64>(14)? as u64,
        suricata_alerts: row.get::<_, i64>(15)? as u64,
        incomplete: row.get(16)?,
        http: HttpMetadata {
            method: row.get(17)?,
            host: row.get(18)?,
            path: row.get(19)?,
            status: row.get::<_, Option<i64>>(20)?.map(|value| value as u16),
            content_type: row.get(21)?,
        },
    })
}

fn push_condition<T>(sql: &mut String, values: &mut Vec<Value>, condition: &str, value: Option<T>)
where
    T: Into<Value>,
{
    if let Some(value) = value {
        sql.push_str(" AND ");
        sql.push_str(condition);
        values.push(value.into());
    }
}

pub fn ip_to_blob(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    }
}

fn blob_to_ip(bytes: &[u8]) -> Result<IpAddr> {
    match bytes.len() {
        4 => Ok(IpAddr::V4(Ipv4Addr::new(
            bytes[0], bytes[1], bytes[2], bytes[3],
        ))),
        16 => {
            let octets: [u8; 16] = bytes.try_into().expect("length checked");
            Ok(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        length => bail!("invalid IP blob length {length}"),
    }
}

fn to_sql_error(error: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Blob,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

pub fn parse_optional_ip(value: Option<&str>) -> Result<Option<IpAddr>> {
    value
        .map(IpAddr::from_str)
        .transpose()
        .context("invalid IP filter")
}
