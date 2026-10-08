//! On-demand session chains. SQL reads are interruptible and payload search is
//! resumable; neither grouping nor chain lifetime adds state to packet workers.
use std::{
    net::IpAddr,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use rusqlite::{params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    catalog::{blob_to_ip, ip_to_blob, map_session_summary, to_sql_error},
    read_payload, Catalog, SessionFilter,
};
use crate::domain::SessionSummary;

const WINDOW_MICROS: i64 = 1_000_000;
const MAX_PAYLOAD_CANDIDATES: usize = 2_000;
const SESSION_COLUMNS: &str = "s.id, s.source_id, src.name, s.started_at, s.ended_at,
    s.client_ip, s.client_port, s.server_ip, s.server_port, s.protocol,
    s.bytes_c2s, s.bytes_s2c, s.contains_flag, s.flag_direction, s.flag_count,
    s.suricata_alerts, s.incomplete, s.http_method, s.http_host, s.http_path,
    s.http_status, s.http_content_type, s.collector_id,
    s.first_payload_c2s_at, s.first_payload_s2c_at";

#[derive(Clone, Debug, Serialize)]
pub struct ChainSummary {
    pub id: i64,
    pub collector_id: Option<String>,
    pub source_id: i64,
    pub source_name: String,
    pub client_ip: IpAddr,
    pub started_at: i64,
    pub ended_at: i64,
    pub session_count: u64,
    pub bytes_c2s: u64,
    pub bytes_s2c: u64,
    pub contains_flag: bool,
    pub flag_count: u64,
    pub suricata_alerts: u64,
    pub incomplete: bool,
    pub snapshot_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub started_at: i64,
    pub id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchPosition {
    pub chain_id: i64,
    pub after: Option<Position>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainCursor {
    pub snapshot: i64,
    pub before: Option<Position>,
    pub search: Option<SearchPosition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberCursor {
    pub snapshot: i64,
    pub after: Position,
}

pub fn decode_cursor<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    if value.len() > 1024 {
        bail!("invalid chain cursor");
    }
    serde_json::from_str(value).context("invalid chain cursor")
}

fn encode_cursor(value: &impl Serialize) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

#[derive(Serialize)]
pub struct ChainPage {
    pub items: Vec<ChainSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct MemberPage {
    pub chain: ChainSummary,
    pub items: Vec<SessionSummary>,
    pub next_cursor: Option<String>,
}

pub struct ChainReader {
    connection: Connection,
    deadline: Instant,
}

impl ChainReader {
    pub fn open(catalog: &Catalog) -> Result<Self> {
        let connection = catalog.connection()?;
        // Keep read work isolated from capture/storage. The deadline and VM budget
        // cover all SQL in a request, including repeated payload-search pages.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut steps = 0usize;
        connection.progress_handler(
            10_000,
            Some(move || {
                steps += 10_000;
                steps >= 50_000_000 || Instant::now() >= deadline
            }),
        );
        connection.busy_timeout(Duration::from_millis(250))?;
        connection.execute_batch("PRAGMA temp_store = FILE; PRAGMA cache_size = -2048; BEGIN;")?;
        Ok(Self {
            connection,
            deadline,
        })
    }

    pub fn snapshot(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row("SELECT COALESCE(MAX(id), 0) FROM sessions", [], |row| {
                row.get(0)
            })?)
    }

    pub fn list(
        &self,
        filter: &SessionFilter,
        cursor: Option<ChainCursor>,
        needle: Option<&[u8]>,
        data_dir: &Path,
    ) -> Result<ChainPage> {
        let mut cursor = cursor.unwrap_or(ChainCursor {
            snapshot: self.snapshot()?,
            before: None,
            search: None,
        });
        if cursor.snapshot < 0 {
            bail!("invalid chain snapshot");
        }
        let limit = filter.limit.clamp(1, 200);
        if needle.is_none() {
            let mut items = self.summaries(filter, &cursor, limit + 1)?;
            let has_more = items.len() > limit;
            items.truncate(limit);
            if let Some(last) = items.last() {
                cursor.before = Some(Position {
                    started_at: last.started_at,
                    id: last.id,
                });
            }
            cursor.search = None;
            return Ok(ChainPage {
                items,
                next_cursor: if has_more {
                    Some(encode_cursor(&cursor)?)
                } else {
                    None
                },
            });
        }

        let needle = needle.expect("checked above");
        let mut items = Vec::with_capacity(limit);
        let mut scanned = 0;
        let mut active_chain: Option<ChainSummary> = None;
        loop {
            if scanned > 0 && Instant::now() >= self.deadline {
                return Ok(ChainPage {
                    items,
                    next_cursor: Some(encode_cursor(&cursor)?),
                });
            }
            let lookup = if let Some(pending) = &cursor.search {
                match &active_chain {
                    Some(chain) if chain.id == pending.chain_id => Ok(Some(chain.clone())),
                    _ => self.summary(pending.chain_id, cursor.snapshot),
                }
            } else {
                self.summaries(filter, &cursor, 1)
                    .map(|items| items.into_iter().next())
            };
            let chain = match lookup {
                Err(error) if scanned > 0 && is_interrupted(&error) => {
                    return Ok(ChainPage {
                        items,
                        next_cursor: Some(encode_cursor(&cursor)?),
                    })
                }
                other => other?,
            };
            let Some(chain) = chain else {
                // A pending chain can disappear through retention. Continue below
                // the saved ordering position rather than restarting its search.
                if cursor.search.take().is_some() {
                    continue;
                }
                return Ok(ChainPage {
                    items,
                    next_cursor: None,
                });
            };
            let after = cursor
                .search
                .as_ref()
                .and_then(|search| search.after.clone());
            active_chain = Some(chain.clone());
            let candidates = self.members(
                chain.id,
                cursor.snapshot,
                after.as_ref(),
                (MAX_PAYLOAD_CANDIDATES - scanned).min(200) + 1,
                Some(filter),
            );
            let candidates = match candidates {
                Err(error) if scanned > 0 && is_interrupted(&error) => {
                    return Ok(ChainPage {
                        items,
                        next_cursor: Some(encode_cursor(&cursor)?),
                    })
                }
                other => other?,
            };
            let has_more = candidates.len() > (MAX_PAYLOAD_CANDIDATES - scanned).min(200);
            let take = (MAX_PAYLOAD_CANDIDATES - scanned).min(200);
            let mut found = false;
            let mut last = after;
            for session in candidates.into_iter().take(take) {
                if scanned > 0 && Instant::now() >= self.deadline {
                    cursor.search = Some(SearchPosition {
                        chain_id: chain.id,
                        after: last,
                    });
                    return Ok(ChainPage {
                        items,
                        next_cursor: Some(encode_cursor(&cursor)?),
                    });
                }
                scanned += 1;
                last = Some(Position {
                    started_at: session.started_at,
                    id: session.id,
                });
                if let Some(stored) = self.stored_session(session.id)? {
                    let payload = read_payload(data_dir.to_path_buf(), &stored)?;
                    if contains(&payload.c2s, needle) || contains(&payload.s2c, needle) {
                        found = true;
                        break;
                    }
                }
            }
            if found || !has_more {
                cursor.before = Some(Position {
                    started_at: chain.started_at,
                    id: chain.id,
                });
                cursor.search = None;
                active_chain = None;
                if found {
                    items.push(chain);
                }
            } else {
                // Save the previous completed chain position as well as progress
                // inside this one. Empty search pages still have a advancing cursor.
                cursor.search = Some(SearchPosition {
                    chain_id: chain.id,
                    after: last,
                });
            }
            if items.len() == limit || scanned == MAX_PAYLOAD_CANDIDATES {
                return Ok(ChainPage {
                    items,
                    next_cursor: Some(encode_cursor(&cursor)?),
                });
            }
        }
    }

    fn stored_session(&self, id: i64) -> Result<Option<crate::domain::StoredSession>> {
        // Read segment addressing within the same SQLite snapshot as membership.
        Ok(self.connection.query_row(&format!("SELECT {SESSION_COLUMNS}, s.segment_id, seg.filename, s.segment_offset, s.record_length FROM sessions s JOIN sources src ON src.id=s.source_id JOIN segments seg ON seg.id=s.segment_id WHERE s.id=?1"), [id], |row| Ok(crate::domain::StoredSession {
            summary: map_session_summary(row)?, segment_id: row.get(25)?, segment_filename: row.get(26)?, segment_offset: row.get::<_, i64>(27)? as u64, record_length: row.get::<_, i64>(28)? as u64,
        })).optional()?)
    }

    pub fn member_page(
        &self,
        id: i64,
        cursor: Option<MemberCursor>,
        snapshot: Option<i64>,
        limit: usize,
    ) -> Result<Option<MemberPage>> {
        let snapshot = match &cursor {
            Some(cursor) => cursor.snapshot,
            None => snapshot.unwrap_or(self.snapshot()?),
        };
        if snapshot < 0 {
            bail!("invalid chain snapshot");
        }
        let Some(chain) = self.summary(id, snapshot)? else {
            return Ok(None);
        };
        let limit = limit.clamp(1, 200);
        let mut items = self.members(
            id,
            snapshot,
            cursor.as_ref().map(|cursor| &cursor.after),
            limit + 1,
            None,
        )?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = if has_more {
            items
                .last()
                .map(|last| {
                    encode_cursor(&MemberCursor {
                        snapshot,
                        after: Position {
                            started_at: last.started_at,
                            id: last.id,
                        },
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(Some(MemberPage {
            chain,
            items,
            next_cursor,
        }))
    }

    fn summaries(
        &self,
        filter: &SessionFilter,
        cursor: &ChainCursor,
        limit: usize,
    ) -> Result<Vec<ChainSummary>> {
        let mut values = vec![Value::Integer(cursor.snapshot)];
        let mut scope = String::new();
        // Only complete key components may prune input before gap detection.
        if let Some(source) = filter.source_id {
            scope.push_str(&format!(" AND source_id = {}", bind(&mut values, source)));
        }
        if let Some(client) = filter.client_ip {
            scope.push_str(&format!(
                " AND client_ip = {}",
                bind(&mut values, Value::Blob(ip_to_blob(client)))
            ));
        }
        let cte = membership_cte(&scope);
        let matched = conditions(filter, &mut values);
        let mut after = String::new();
        if let Some(before) = &cursor.before {
            let time = bind(&mut values, before.started_at);
            let id = bind(&mut values, before.id);
            after = format!(" WHERE (started_at, id) < ({time}, {id})");
        }
        let limit = bind(&mut values, limit as i64);
        let sql = format!("{cte}, summaries AS ({}) SELECT * FROM summaries{after} ORDER BY started_at DESC, id DESC LIMIT {limit}", aggregate(&matched, None));
        self.query_summaries(&sql, values, cursor.snapshot)
    }

    pub fn summary(&self, id: i64, snapshot: i64) -> Result<Option<ChainSummary>> {
        let Some(scope) = self.anchor_scope(id, snapshot)? else {
            return Ok(None);
        };
        let values = vec![Value::Integer(snapshot), Value::Integer(id)];
        let sql = format!(
            "{} {}",
            membership_cte(&scope),
            aggregate(
                "1",
                Some("s.chain_start = (SELECT chain_start FROM members WHERE id=?2)")
            )
        );
        Ok(self
            .query_summaries(&sql, values, snapshot)?
            .into_iter()
            .next())
    }

    fn query_summaries(
        &self,
        sql: &str,
        values: Vec<Value>,
        snapshot: i64,
    ) -> Result<Vec<ChainSummary>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok(ChainSummary {
                id: row.get(0)?,
                collector_id: row.get(1)?,
                source_id: row.get(2)?,
                source_name: row.get(3)?,
                client_ip: blob_to_ip(&row.get::<_, Vec<u8>>(4)?).map_err(to_sql_error)?,
                started_at: row.get(5)?,
                ended_at: row.get(6)?,
                session_count: row.get::<_, i64>(7)? as u64,
                bytes_c2s: row.get::<_, i64>(8)? as u64,
                bytes_s2c: row.get::<_, i64>(9)? as u64,
                contains_flag: row.get(10)?,
                flag_count: row.get::<_, i64>(11)? as u64,
                suricata_alerts: row.get::<_, i64>(12)? as u64,
                incomplete: row.get(13)?,
                snapshot_id: snapshot,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn anchor_scope(&self, id: i64, snapshot: i64) -> Result<Option<String>> {
        let exists = self
            .connection
            .query_row(
                "SELECT 1 FROM sessions WHERE id=?1 AND id<=?2",
                [id, snapshot],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(exists.then(|| " AND ((collector_id IS NOT NULL AND collector_id = (SELECT collector_id FROM sessions WHERE id=?2) AND client_ip = (SELECT client_ip FROM sessions WHERE id=?2) AND source_id = (SELECT source_id FROM sessions WHERE id=?2)) OR (collector_id IS NULL AND id=?2))".to_owned()))
    }

    fn members(
        &self,
        id: i64,
        snapshot: i64,
        after: Option<&Position>,
        limit: usize,
        filter: Option<&SessionFilter>,
    ) -> Result<Vec<SessionSummary>> {
        let Some(scope) = self.anchor_scope(id, snapshot)? else {
            return Ok(Vec::new());
        };
        let mut values = vec![Value::Integer(snapshot), Value::Integer(id)];
        let mut extra = String::new();
        if let Some(after) = after {
            let time = bind(&mut values, after.started_at);
            let id = bind(&mut values, after.id);
            extra.push_str(&format!(" AND (s.started_at, s.id) > ({time}, {id})"));
        }
        if let Some(filter) = filter {
            extra.push_str(&format!(" AND ({})", conditions(filter, &mut values)));
        }
        let limit = bind(&mut values, limit as i64);
        let sql = format!("{} SELECT {SESSION_COLUMNS} FROM members s JOIN sources src ON src.id=s.source_id WHERE s.chain_start=(SELECT chain_start FROM members WHERE id=?2){extra} ORDER BY s.started_at, s.id LIMIT {limit}", membership_cte(&scope));
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), map_session_summary)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn bind(values: &mut Vec<Value>, value: impl Into<Value>) -> String {
    values.push(value.into());
    format!("?{}", values.len())
}

fn conditions(filter: &SessionFilter, values: &mut Vec<Value>) -> String {
    let mut result = vec!["1".to_owned()];
    for (column, value) in [
        ("source_id", filter.source_id.map(Value::Integer)),
        (
            "contains_flag",
            filter.contains_flag.map(|v| Value::Integer(i64::from(v))),
        ),
        (
            "protocol",
            filter.protocol.map(|v| Value::Integer(v as i64)),
        ),
        (
            "client_ip",
            filter.client_ip.map(|v| Value::Blob(ip_to_blob(v))),
        ),
        (
            "server_ip",
            filter.server_ip.map(|v| Value::Blob(ip_to_blob(v))),
        ),
        (
            "client_port",
            filter.client_port.map(|v| Value::Integer(i64::from(v))),
        ),
        (
            "server_port",
            filter.server_port.map(|v| Value::Integer(i64::from(v))),
        ),
    ] {
        if let Some(value) = value {
            result.push(format!("s.{column} = {}", bind(values, value)));
        }
    }
    if let Some(value) = filter.started_after {
        result.push(format!("s.started_at >= {}", bind(values, value)));
    }
    result.join(" AND ")
}

fn membership_cte(scope: &str) -> String {
    // NULL collector identity is partitioned by session ID, making legacy rows
    // singletons. Gaps are calculated before flag/protocol/payload filtering.
    format!("WITH ordered AS (
        SELECT *, CASE WHEN collector_id IS NULL THEN id ELSE 0 END AS legacy_id,
            LAG(started_at) OVER (PARTITION BY collector_id, client_ip, source_id,
                CASE WHEN collector_id IS NULL THEN id ELSE 0 END ORDER BY started_at, id) AS previous_start
        FROM sessions WHERE id<=?1 AND (protocol!=0 OR bytes_c2s!=0 OR bytes_s2c!=0){scope}
    ), boundaries AS (
        SELECT *, CASE WHEN previous_start IS NULL OR started_at-previous_start>{WINDOW_MICROS} THEN started_at END AS boundary FROM ordered
    ), members AS (
        SELECT *, MAX(boundary) OVER (PARTITION BY collector_id, client_ip, source_id, legacy_id ORDER BY started_at, id ROWS UNBOUNDED PRECEDING) AS chain_start FROM boundaries
    )")
}

fn aggregate(matched: &str, target: Option<&str>) -> String {
    let target = target.map_or(String::new(), |value| format!(" WHERE {value}"));
    format!("SELECT MIN(s.id) AS id, s.collector_id, s.source_id, src.name AS source_name, s.client_ip,
        MIN(s.started_at) AS started_at, MAX(s.ended_at) AS ended_at, COUNT(*) AS session_count,
        SUM(s.bytes_c2s) AS bytes_c2s, SUM(s.bytes_s2c) AS bytes_s2c, MAX(s.contains_flag) AS contains_flag,
        SUM(s.flag_count) AS flag_count, SUM(s.suricata_alerts) AS suricata_alerts, MAX(s.incomplete) AS incomplete
        FROM members s JOIN sources src ON src.id=s.source_id{target}
        GROUP BY s.collector_id, s.client_ip, s.source_id, s.legacy_id, s.chain_start HAVING MAX({matched}) != 0")
}

pub fn is_interrupted(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| matches!(cause.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::OperationInterrupted))
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || bytes.windows(needle.len()).any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::SourceInput, storage::record::append_record};
    use std::fs::OpenOptions;

    struct Fixture {
        directory: tempfile::TempDir,
        catalog: Catalog,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let catalog = Catalog::open(directory.path().join("index.sqlite")).unwrap();
            for (name, port) in [("web", 8080), ("other", 9000)] {
                catalog
                    .create_source(&SourceInput {
                        name: name.into(),
                        port,
                        enabled: true,
                    })
                    .unwrap();
            }
            catalog
                .connection()
                .unwrap()
                .execute(
                    "INSERT INTO segments(id,filename,created_at) VALUES(1,'test.seg',0)",
                    [],
                )
                .unwrap();
            Self { directory, catalog }
        }
        fn add(
            &self,
            id: i64,
            start: i64,
            collector: Option<&str>,
            client: &str,
            source: i64,
            flag: bool,
        ) {
            let mut segment = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(self.directory.path().join("segments/test.seg"))
                .unwrap();
            let request = format!("GET /{id} HTTP/1.1\r\nHost: test\r\n\r\n");
            let reply = if flag {
                b"FLAG{test}".as_slice()
            } else {
                b"response"
            };
            let record = append_record(&mut segment, id, request.as_bytes(), reply).unwrap();
            self.catalog.connection().unwrap().execute(
                "INSERT INTO sessions(id,segment_id,segment_offset,record_length,started_at,ended_at,source_id,client_ip,client_port,server_ip,server_port,protocol,bytes_c2s,bytes_s2c,contains_flag,flag_count,collector_id,first_payload_c2s_at,first_payload_s2c_at) VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9,8080,1,?10,?11,?12,?12,?13,?4,?5)",
                rusqlite::params![id, record.offset as i64, record.length as i64, start, start+2_000_000, source, ip_to_blob(client.parse().unwrap()), 40_000 + id % 1000, ip_to_blob("10.0.0.1".parse().unwrap()), request.len() as i64, reply.len() as i64, flag, collector],
            ).unwrap();
        }
        fn reader(&self) -> ChainReader {
            ChainReader::open(&self.catalog).unwrap()
        }
        fn list(&self, filter: &SessionFilter) -> ChainPage {
            self.reader()
                .list(filter, None, None, self.directory.path())
                .unwrap()
        }
    }

    #[test]
    fn sliding_window_uses_starts_and_all_key_components() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.directory.path().join("segments")).unwrap();
        f.add(1, 0, Some("one"), "10.0.0.2", 1, false);
        f.add(2, 800_000, Some("one"), "10.0.0.2", 1, false);
        f.add(3, 1_600_000, Some("one"), "10.0.0.2", 1, true);
        f.add(4, 2_600_000, Some("one"), "10.0.0.2", 1, false);
        f.add(5, 3_600_001, Some("one"), "10.0.0.2", 1, false);
        f.add(6, 800_000, Some("two"), "10.0.0.2", 1, false);
        f.add(7, 800_000, Some("one"), "10.0.0.3", 1, false);
        f.add(8, 800_000, Some("one"), "10.0.0.2", 2, false);
        f.add(9, 800_000, None, "10.0.0.2", 1, false);
        f.add(10, 800_000, None, "10.0.0.2", 1, false);
        let page = f.list(&SessionFilter {
            limit: 100,
            ..Default::default()
        });
        assert_eq!(page.items.len(), 7);
        let chain = page.items.iter().find(|chain| chain.id == 1).unwrap();
        assert_eq!(chain.session_count, 4);
        assert_eq!(chain.started_at, 0);
        assert!(chain.contains_flag);
        assert_eq!(chain.flag_count, 1);
        assert!(page
            .items
            .iter()
            .filter(|chain| chain.collector_id.is_none())
            .all(|chain| chain.session_count == 1));
    }

    #[test]
    fn filters_preserve_context_and_member_pages_keep_whole_chain() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.directory.path().join("segments")).unwrap();
        for id in 1..=3 {
            f.add(id, (id - 1) * 800_000, Some("one"), "::1", 1, id == 3);
        }
        let filter = SessionFilter {
            contains_flag: Some(true),
            limit: 1,
            ..Default::default()
        };
        let page = f.list(&filter);
        assert_eq!(page.items[0].session_count, 3);
        assert_eq!(page.items[0].started_at, 0);
        let reader = f.reader();
        let mut cursor = None;
        let mut ids = Vec::new();
        loop {
            let page = reader.member_page(1, cursor, None, 1).unwrap().unwrap();
            ids.extend(page.items.iter().map(|member| member.id));
            cursor = page
                .next_cursor
                .as_deref()
                .map(decode_cursor::<MemberCursor>)
                .transpose()
                .unwrap();
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids, vec![1, 2, 3]);
        let payload = f
            .reader()
            .list(&filter, None, Some(b"FLAG{test}"), f.directory.path())
            .unwrap();
        assert_eq!(payload.items[0].session_count, 3);
        assert_eq!(payload.items[0].id, 1);
        let no_match = f
            .reader()
            .list(&filter, None, Some(b"/1 HTTP"), f.directory.path())
            .unwrap();
        assert!(
            no_match.items.is_empty(),
            "all active filters must match the same member"
        );
    }

    #[test]
    fn late_bridge_merges_chains_but_snapshot_pagination_stays_stable() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.directory.path().join("segments")).unwrap();
        f.add(1, 2_000_000, Some("one"), "10.0.0.2", 1, false);
        f.add(2, 0, Some("one"), "10.0.0.2", 1, false);
        let filter = SessionFilter {
            limit: 1,
            ..Default::default()
        };
        let first = f.list(&filter);
        let cursor = decode_cursor::<ChainCursor>(first.next_cursor.as_deref().unwrap()).unwrap();
        f.add(3, 1_000_000, Some("one"), "10.0.0.2", 1, false);
        let older = f
            .reader()
            .list(&filter, Some(cursor), None, f.directory.path())
            .unwrap();
        assert_eq!(older.items[0].id, 2);
        assert_eq!(older.items[0].session_count, 1);
        let refreshed = f.list(&filter);
        assert_eq!(refreshed.items.len(), 1);
        assert_eq!(refreshed.items[0].session_count, 3);
        assert_eq!(refreshed.items[0].started_at, 0);
        let members = f.reader().member_page(1, None, None, 20).unwrap().unwrap();
        assert_eq!(
            members
                .items
                .iter()
                .map(|member| member.id)
                .collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
        f.catalog
            .connection()
            .unwrap()
            .execute("DELETE FROM sessions WHERE id=2", [])
            .unwrap();
        assert_eq!(f.list(&filter).items[0].session_count, 2);
    }

    #[test]
    fn payload_search_resumes_inside_a_chain_after_candidate_budget() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.directory.path().join("segments")).unwrap();
        for id in 1..=2002 {
            f.add(id, id * 100, Some("one"), "10.0.0.2", 1, false);
        }
        let filter = SessionFilter {
            limit: 10,
            ..Default::default()
        };
        let first = f
            .reader()
            .list(&filter, None, Some(b"GET /2001 HTTP"), f.directory.path())
            .unwrap();
        assert!(first.items.is_empty());
        let cursor = decode_cursor::<ChainCursor>(first.next_cursor.as_deref().unwrap()).unwrap();
        assert_eq!(
            cursor.search.as_ref().unwrap().after.as_ref().unwrap().id,
            2000
        );
        let second = f
            .reader()
            .list(
                &filter,
                Some(cursor),
                Some(b"GET /2001 HTTP"),
                f.directory.path(),
            )
            .unwrap();
        assert_eq!(second.items[0].session_count, 2002);
        assert_eq!(second.items[0].id, 1);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn equal_times_have_deterministic_members_and_chain_pagination() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.directory.path().join("segments")).unwrap();
        for id in 1..=5 {
            f.add(id, 0, Some("one"), "10.0.0.2", 1, false);
        }
        f.add(6, 0, Some("two"), "10.0.0.2", 1, false);
        let filter = SessionFilter {
            limit: 1,
            ..Default::default()
        };
        let first = f.list(&filter);
        assert_eq!(first.items[0].id, 6);
        let cursor = decode_cursor(first.next_cursor.as_deref().unwrap()).unwrap();
        let second = f
            .reader()
            .list(&filter, Some(cursor), None, f.directory.path())
            .unwrap();
        assert_eq!(second.items[0].id, 1);
        assert!(second.next_cursor.is_none());
        let members = f.reader().member_page(1, None, None, 20).unwrap().unwrap();
        assert_eq!(
            members
                .items
                .iter()
                .map(|member| member.id)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn expensive_read_is_interrupted_without_affecting_storage_or_next_reader() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.directory.path().join("segments")).unwrap();
        let reader = f.reader();
        let error = reader.connection.query_row(
            "WITH RECURSIVE numbers(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM numbers WHERE x<10000000) SELECT SUM(x) FROM numbers",
            [], |row| row.get::<_, i64>(0),
        ).unwrap_err();
        assert!(is_interrupted(&error.into()));
        drop(reader);
        f.add(1, 0, Some("one"), "10.0.0.2", 1, false);
        assert_eq!(f.list(&SessionFilter::default()).items.len(), 1);
    }
}
