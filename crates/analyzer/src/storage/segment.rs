use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::domain::SessionPayload;

use super::record::{append_record, read_record, RecordLocation};

pub struct SegmentManager {
    directory: PathBuf,
    duration_micros: i64,
    retention_count: usize,
    active: Option<ActiveSegment>,
}

struct ActiveSegment {
    id: i64,
    created_at: i64,
    size_bytes: u64,
    file: File,
}

#[derive(Clone, Copy, Debug)]
pub struct StoredLocation {
    pub segment_id: i64,
    pub offset: u64,
    pub length: u64,
}

impl SegmentManager {
    pub fn new(data_dir: &Path, duration: Duration, retention_count: usize) -> Result<Self> {
        let directory = data_dir.join("segments");
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("create segment directory {}", directory.display()))?;
        Ok(Self {
            directory,
            duration_micros: duration.as_micros().min(i64::MAX as u128) as i64,
            retention_count,
            active: None,
        })
    }

    pub fn append(
        &mut self,
        connection: &Connection,
        now_micros: i64,
        session_id: i64,
        c2s: &[u8],
        s2c: &[u8],
    ) -> Result<StoredLocation> {
        self.ensure_active(connection, now_micros)?;
        let active = self.active.as_mut().expect("active segment ensured");
        let location = append_record(&mut active.file, session_id, c2s, s2c)?;
        active.size_bytes = location.offset + location.length;
        Ok(StoredLocation {
            segment_id: active.id,
            offset: location.offset,
            length: location.length,
        })
    }

    fn ensure_active(&mut self, connection: &Connection, now_micros: i64) -> Result<()> {
        let rotate = self
            .active
            .as_ref()
            .map(|segment| now_micros.saturating_sub(segment.created_at) >= self.duration_micros)
            .unwrap_or(true);
        if !rotate {
            return Ok(());
        }

        if let Some(mut old) = self.active.take() {
            old.file.flush().context("flush closing segment")?;
            old.file.sync_data().context("sync closing segment")?;
            connection.execute(
                "UPDATE segments SET closed_at = ?1, size_bytes = ?2 WHERE id = ?3",
                params![now_micros, old.size_bytes, old.id],
            )?;
        }

        let filename = format!("{now_micros}.seg");
        let path = self.directory.join(&filename);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open segment {}", path.display()))?;
        file.seek(SeekFrom::End(0))?;
        let size_bytes = file.metadata()?.len();
        connection.execute(
            "INSERT INTO segments(filename, created_at, size_bytes) VALUES (?1, ?2, ?3)",
            params![filename, now_micros, size_bytes],
        )?;
        let id = connection.last_insert_rowid();
        self.active = Some(ActiveSegment {
            id,
            created_at: now_micros,
            size_bytes,
            file,
        });
        self.expire_old_segments(connection)?;
        Ok(())
    }

    pub fn flush_active(&mut self, connection: &Connection) -> Result<()> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.file.flush().context("flush active segment")?;
        active.file.sync_data().context("sync active segment")?;
        connection.execute(
            "UPDATE segments SET size_bytes = ?1 WHERE id = ?2",
            params![active.size_bytes, active.id],
        )?;
        Ok(())
    }

    fn expire_old_segments(&self, connection: &Connection) -> Result<()> {
        let mut statement = connection.prepare(
            "SELECT id, filename FROM segments ORDER BY created_at DESC LIMIT -1 OFFSET ?1",
        )?;
        let expired = statement
            .query_map([self.retention_count as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        for (id, filename) in expired {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute("DELETE FROM sessions WHERE segment_id = ?1", [id])?;
            transaction.execute("DELETE FROM segments WHERE id = ?1", [id])?;
            transaction.commit()?;
            let path = self.directory.join(filename);
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), %error, "failed to remove expired segment file");
                }
            }
        }
        Ok(())
    }

    pub fn read_payload(
        data_dir: &Path,
        filename: &str,
        session_id: i64,
        offset: u64,
        length: u64,
    ) -> Result<SessionPayload> {
        if Path::new(filename)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(filename)
        {
            anyhow::bail!("invalid segment filename");
        }
        let path = data_dir.join("segments").join(filename);
        let mut file = File::open(&path)
            .with_context(|| format!("open segment payload {}", path.display()))?;
        read_record(&mut file, session_id, RecordLocation { offset, length })
    }
}
