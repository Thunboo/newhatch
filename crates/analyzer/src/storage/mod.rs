mod catalog;
mod record;
mod schema;
mod segment;

use std::{path::PathBuf, thread, time::Duration};

use anyhow::{Context, Result};
use rusqlite::params;
use tokio::sync::mpsc;

use crate::domain::{CompletedSession, SessionPayload};

use catalog::ip_to_blob;
pub use catalog::{parse_optional_ip, Catalog, SessionFilter};
use segment::SegmentManager;

#[derive(Clone)]
pub struct StorageHandle {
    sender: mpsc::Sender<CompletedSession>,
}

impl StorageHandle {
    pub fn try_store(&self, session: CompletedSession) -> bool {
        self.sender.try_send(session).is_ok()
    }
}

pub fn start_writer(
    catalog: Catalog,
    data_dir: PathBuf,
    segment_duration: Duration,
    retention_count: usize,
    queue_capacity: usize,
) -> Result<StorageHandle> {
    let (sender, receiver) = mpsc::channel(queue_capacity);
    thread::Builder::new()
        .name("storage-writer".to_owned())
        .spawn(move || {
            if let Err(error) = run_writer(
                catalog,
                data_dir,
                segment_duration,
                retention_count,
                receiver,
            ) {
                tracing::error!(error = ?error, "storage writer stopped");
            }
        })
        .context("spawn storage writer")?;
    Ok(StorageHandle { sender })
}

fn run_writer(
    catalog: Catalog,
    data_dir: PathBuf,
    segment_duration: Duration,
    retention_count: usize,
    mut receiver: mpsc::Receiver<CompletedSession>,
) -> Result<()> {
    let mut connection = catalog.connection()?;
    let mut segments = SegmentManager::new(&data_dir, segment_duration, retention_count)?;
    let mut next_session_id: i64 =
        connection.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM sessions", [], |row| {
            row.get(0)
        })?;

    while let Some(first) = receiver.blocking_recv() {
        let mut batch = Vec::with_capacity(500);
        batch.push(first);
        while batch.len() < 500 {
            match receiver.try_recv() {
                Ok(session) => batch.push(session),
                Err(_) => break,
            }
        }

        let mut pending_rows = Vec::with_capacity(batch.len());
        for session in batch {
            let id = next_session_id;
            next_session_id = next_session_id.saturating_add(1);
            let location = segments.append(
                &connection,
                session.ended_at,
                id,
                &session.c2s,
                &session.s2c,
            )?;
            pending_rows.push((id, session, location));
        }
        segments.flush_active(&connection)?;

        let transaction = connection.transaction()?;
        for (id, session, location) in pending_rows {
            transaction.execute(
                r#"
                INSERT INTO sessions(
                    id, segment_id, segment_offset, record_length,
                    started_at, ended_at, source_id,
                    client_ip, client_port, server_ip, server_port,
                    protocol, bytes_c2s, bytes_s2c,
                    contains_flag, flag_direction, flag_count,
                    incomplete, http_method, http_host, http_path,
                    http_status, http_content_type
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                    ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                    ?22, ?23
                )
                "#,
                params![
                    id,
                    location.segment_id,
                    location.offset as i64,
                    location.length as i64,
                    session.started_at,
                    session.ended_at,
                    session.source_id,
                    ip_to_blob(session.client_ip),
                    session.client_port,
                    ip_to_blob(session.server_ip),
                    session.server_port,
                    session.protocol as i64,
                    session.c2s.len() as i64,
                    session.s2c.len() as i64,
                    session.flag_count > 0,
                    session.flag_direction as i64,
                    session.flag_count as i64,
                    session.incomplete,
                    session.http.method,
                    session.http.host,
                    session.http.path,
                    session.http.status,
                    session.http.content_type,
                ],
            )?;
        }
        transaction.commit()?;
    }
    Ok(())
}

pub fn read_payload(
    data_dir: PathBuf,
    stored: &crate::domain::StoredSession,
) -> Result<SessionPayload> {
    SegmentManager::read_payload(
        &data_dir,
        &stored.segment_filename,
        stored.summary.id,
        stored.segment_offset,
        stored.record_length,
    )
}

#[cfg(test)]
mod tests {
    use std::{net::IpAddr, str::FromStr, time::Duration};

    use crate::domain::{
        CompletedSession, FlagDirection, HttpMetadata, SessionProtocol, SourceInput,
    };

    use super::{read_payload, run_writer, Catalog, SessionFilter};

    #[test]
    fn writer_persists_metadata_and_payload_in_separate_stores() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path().join("index.sqlite")).unwrap();
        let source = catalog
            .create_source(&SourceInput {
                name: "web".to_owned(),
                port: 8080,
                enabled: true,
            })
            .unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let writer_catalog = catalog.clone();
        let data_dir = directory.path().to_path_buf();
        let writer_data_dir = data_dir.clone();
        let writer = std::thread::spawn(move || {
            run_writer(
                writer_catalog,
                writer_data_dir,
                Duration::from_secs(60),
                3,
                receiver,
            )
            .unwrap();
        });

        sender
            .blocking_send(CompletedSession {
                source_id: source.id,
                started_at: 1,
                ended_at: 2,
                client_ip: IpAddr::from_str("10.0.0.2").unwrap(),
                client_port: 50_000,
                server_ip: IpAddr::from_str("10.0.0.1").unwrap(),
                server_port: 8080,
                protocol: SessionProtocol::Http,
                c2s: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                s2c: b"HTTP/1.1 200 OK\r\n\r\nFLAG_TEST".to_vec(),
                flag_direction: FlagDirection::S2c,
                flag_count: 1,
                incomplete: false,
                http: HttpMetadata::default(),
            })
            .unwrap();
        drop(sender);
        writer.join().unwrap();

        let sessions = catalog
            .list_sessions(&SessionFilter {
                limit: 10,
                ..SessionFilter::default()
            })
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].contains_flag);
        let stored = catalog.get_session(sessions[0].id).unwrap().unwrap();
        let payload = read_payload(data_dir, &stored).unwrap();
        assert_eq!(payload.c2s, b"GET / HTTP/1.1\r\n\r\n");
        assert!(payload.s2c.ends_with(b"FLAG_TEST"));
    }
}
