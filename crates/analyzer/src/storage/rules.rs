use super::Catalog;
use crate::attack_rules::{AttackRule, RuleInput};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

pub(crate) fn read_rules(
    connection: &rusqlite::Connection,
    owner: &str,
) -> Result<Vec<AttackRule>> {
    let mut statement =
        connection.prepare("SELECT id, definition FROM attack_rules WHERE owner=?1 ORDER BY id")?;
    let rows = statement.query_map([owner], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (id, json) = row?;
        Ok(AttackRule {
            id,
            owner: owner.to_owned(),
            input: serde_json::from_str(&json)?,
        })
    })
    .collect()
}

impl Catalog {
    pub fn list_rules(&self, owner: &str) -> Result<Vec<AttackRule>> {
        let connection = self.connection()?;
        read_rules(&connection, owner)
    }

    pub fn save_rule(
        &self,
        owner: &str,
        id: Option<i64>,
        input: RuleInput,
    ) -> Result<Option<AttackRule>> {
        let input = crate::attack_rules::validate(input)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE id=?1)",
            [input.source_id],
            |row| row.get(0),
        )?;
        ensure!(source, "source not found");
        let definition = serde_json::to_string(&input)?;
        let id = if let Some(id) = id {
            if transaction.execute(
                "UPDATE attack_rules SET source_id=?1, definition=?2 WHERE id=?3 AND owner=?4",
                params![input.source_id, definition, id, owner],
            )? == 0
            {
                return Ok(None);
            }
            id
        } else {
            let count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM attack_rules WHERE owner=?1",
                [owner],
                |r| r.get(0),
            )?;
            ensure!(count < 100, "at most 100 rules per user");
            transaction.execute(
                "INSERT INTO attack_rules(owner,source_id,definition) VALUES(?1,?2,?3)",
                params![owner, input.source_id, definition],
            )?;
            transaction.last_insert_rowid()
        };
        transaction.commit()?;
        Ok(Some(AttackRule {
            id,
            owner: owner.to_owned(),
            input,
        }))
    }

    pub fn delete_rule(&self, owner: &str, id: i64) -> Result<bool> {
        Ok(self.connection()?.execute(
            "DELETE FROM attack_rules WHERE owner=?1 AND id=?2",
            params![owner, id],
        )? > 0)
    }

    /// Run before readers/capture start. Never mix old byte-based flag metadata
    /// with readable-text Flags-only queries. Segment contents are not rewritten.
    pub fn refresh_flag_metadata(
        &self,
        data_dir: &std::path::Path,
        regex: &regex::bytes::Regex,
    ) -> Result<()> {
        let revision = format!("{}:{}", crate::payload_text::VERSION, regex.as_str());
        let mut connection = self.connection()?;
        let previous: Option<String> = connection
            .query_row(
                "SELECT value FROM catalog_settings WHERE key='flag_text_revision'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if previous.as_deref() == Some(&revision) {
            return Ok(());
        }
        tracing::info!(
            representation = crate::payload_text::VERSION,
            "rebuilding retained flag metadata"
        );
        let mut after = 0;
        let mut scanned = 0usize;
        loop {
            let ids = {
                let mut statement = connection
                    .prepare("SELECT id FROM sessions WHERE id>?1 ORDER BY id LIMIT 200")?;
                let rows = statement.query_map([after], |r| r.get::<_, i64>(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            if ids.is_empty() {
                break;
            }
            let mut updates = Vec::new();
            for id in ids {
                after = id;
                scanned += 1;
                let Some(stored) = self.get_session(id)? else {
                    continue;
                };
                let payload = super::read_payload(data_dir.to_path_buf(), &stored)
                    .context("reindex readable flag metadata")?;
                let c2s = crate::payload_text::flag_count(regex, &payload.c2s);
                let s2c = crate::payload_text::flag_count(regex, &payload.s2c);
                updates.push((
                    id,
                    c2s + s2c,
                    crate::domain::FlagDirection::from_counts(c2s, s2c),
                ));
            }
            let transaction = connection.transaction()?;
            for (id, count, direction) in updates {
                transaction.execute("UPDATE sessions SET contains_flag=?1, flag_count=?2, flag_direction=?3 WHERE id=?4", params![count > 0, count as i64, direction as i64, id])?;
            }
            transaction.commit()?;
        }
        connection.execute("INSERT INTO catalog_settings(key,value) VALUES('flag_text_revision',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [revision])?;
        tracing::info!(
            sessions = scanned,
            "retained flag metadata rebuild complete"
        );
        Ok(())
    }
}
