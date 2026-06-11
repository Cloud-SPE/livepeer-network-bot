//! Repo for the `reward_events`, `delegator_events`, and `delegator_history`
//! tables introduced in migration 0003.

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::domains::explorer::types::{EventRow, TranscoderParamsRow};

#[derive(Debug, Clone)]
pub struct RewardEventRow {
    pub id: String,
    pub tx_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub orch_address: String,
    pub amount_native: Option<String>,
    pub amount_usd: Option<String>,
    pub native_usd_price: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DelegatorEventRow {
    pub id: String,
    pub event_name: String,
    pub tx_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub delegator_address: String,
    pub orch_address: String,
    pub amount_native: Option<String>,
    pub amount_usd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CutChangeEventRow {
    pub event_id: String,
    pub tx_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub orch_address: String,
    pub reward_cut_percent: String,
    pub fee_share_percent: String,
    pub fee_cut_percent: String,
}

#[derive(Debug)]
pub struct EventStreamsRepo {
    pool: SqlitePool,
}

impl EventStreamsRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ---- reward_events -----------------------------------------------------

    /// Returns `true` if this is the first time we've seen this event.
    pub async fn insert_reward_event(&self, ev: &EventRow) -> anyhow::Result<bool> {
        let Some(orch) = ev.to_address.as_deref() else {
            return Ok(false);
        };
        let val = ev.valuations.as_ref().and_then(|v| v.first());
        let amount_usd = val.and_then(|v| v.amount_usd.clone());
        let native_usd_price = val.and_then(|v| v.native_usd_price.clone());

        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO reward_events (
                id, chain_id, tx_hash, log_index, block_number, block_timestamp,
                orch_address, amount_native, amount_usd, native_usd_price, fetched_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&ev.id)
        .bind(&ev.chain_id)
        .bind(&ev.tx_hash)
        .bind(ev.log_index)
        .bind(&ev.block_number)
        .bind(ev.block_timestamp)
        .bind(orch)
        .bind(&ev.amount_native)
        .bind(&amount_usd)
        .bind(&native_usd_price)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn fetch_unsent_reward_events(
        &self,
        limit: i64,
    ) -> anyhow::Result<Vec<RewardEventRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tx_hash, block_timestamp, orch_address,
                   amount_native, amount_usd, native_usd_price
            FROM reward_events
            WHERE sent_to_subscribers_at IS NULL
            ORDER BY block_timestamp ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RewardEventRow {
                id: r.get(0),
                tx_hash: r.get(1),
                block_timestamp: r.get(2),
                orch_address: r.get(3),
                amount_native: r.get(4),
                amount_usd: r.get(5),
                native_usd_price: r.get(6),
            })
            .collect())
    }

    pub async fn mark_reward_event_sent(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE reward_events SET sent_to_subscribers_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- cut_change_events -------------------------------------------------

    pub async fn has_cut_change_events_for_orch(&self, orch_address: &str) -> anyhow::Result<bool> {
        let row = sqlx::query("SELECT 1 FROM cut_change_events WHERE orch_address = ? LIMIT 1")
            .bind(orch_address)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    pub async fn insert_cut_change_event(
        &self,
        ev: &TranscoderParamsRow,
        mark_sent: bool,
    ) -> anyhow::Result<bool> {
        let sent_at = mark_sent.then(Utc::now);
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO cut_change_events (
                event_id, tx_hash, log_index, block_number, block_timestamp,
                orch_address, reward_cut_percent, fee_share_percent,
                fee_cut_percent, fetched_at, sent_to_subscribers_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&ev.event_id)
        .bind(&ev.tx_hash)
        .bind(ev.log_index)
        .bind(&ev.block_number)
        .bind(ev.block_timestamp)
        .bind(&ev.transcoder_address)
        .bind(&ev.reward_cut_percent)
        .bind(&ev.fee_share_percent)
        .bind(&ev.fee_cut_percent)
        .bind(Utc::now())
        .bind(sent_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn fetch_unsent_cut_change_events(
        &self,
        limit: i64,
    ) -> anyhow::Result<Vec<CutChangeEventRow>> {
        let rows = sqlx::query(
            r#"
            SELECT event_id, tx_hash, block_timestamp, orch_address,
                   reward_cut_percent, fee_share_percent, fee_cut_percent
            FROM cut_change_events
            WHERE sent_to_subscribers_at IS NULL
            ORDER BY block_timestamp ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| CutChangeEventRow {
                event_id: r.get(0),
                tx_hash: r.get(1),
                block_timestamp: r.get(2),
                orch_address: r.get(3),
                reward_cut_percent: r.get(4),
                fee_share_percent: r.get(5),
                fee_cut_percent: r.get(6),
            })
            .collect())
    }

    pub async fn mark_cut_change_event_sent(&self, event_id: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE cut_change_events SET sent_to_subscribers_at = ? WHERE event_id = ?")
            .bind(Utc::now())
            .bind(event_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- delegator_events --------------------------------------------------

    pub async fn insert_delegator_event(&self, ev: &EventRow) -> anyhow::Result<bool> {
        let Some(delegator) = ev.from_address.as_deref() else {
            return Ok(false);
        };
        let Some(orch) = ev.to_address.as_deref() else {
            return Ok(false);
        };
        let val = ev.valuations.as_ref().and_then(|v| v.first());
        let amount_usd = val.and_then(|v| v.amount_usd.clone());

        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO delegator_events (
                id, event_name, chain_id, tx_hash, log_index, block_number,
                block_timestamp, delegator_address, orch_address,
                amount_native, amount_usd, fetched_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&ev.id)
        .bind(&ev.event_name)
        .bind(&ev.chain_id)
        .bind(&ev.tx_hash)
        .bind(ev.log_index)
        .bind(&ev.block_number)
        .bind(ev.block_timestamp)
        .bind(delegator)
        .bind(orch)
        .bind(&ev.amount_native)
        .bind(&amount_usd)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn fetch_unsent_delegator_events(
        &self,
        limit: i64,
    ) -> anyhow::Result<Vec<DelegatorEventRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, event_name, tx_hash, block_timestamp,
                   delegator_address, orch_address, amount_native, amount_usd
            FROM delegator_events
            WHERE sent_to_subscribers = 0
            ORDER BY block_timestamp ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| DelegatorEventRow {
                id: r.get(0),
                event_name: r.get(1),
                tx_hash: r.get(2),
                block_timestamp: r.get(3),
                delegator_address: r.get(4),
                orch_address: r.get(5),
                amount_native: r.get(6),
                amount_usd: r.get(7),
            })
            .collect())
    }

    pub async fn mark_delegator_events_sent(&self, ids: &[String]) -> anyhow::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for id in ids {
            sqlx::query("UPDATE delegator_events SET sent_to_subscribers = 1 WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Count delegator_events for the same `(delegator, orch)` pair with
    /// `block_timestamp` strictly before `before`. Used by the subscriber
    /// digest to distinguish a "new delegator" Bond (count == 0) from a
    /// subsequent "stake change" Bond.
    pub async fn count_prior_delegator_events(
        &self,
        delegator: &str,
        orch: &str,
        before: DateTime<Utc>,
    ) -> anyhow::Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) FROM delegator_events
            WHERE delegator_address = ?
              AND orch_address = ?
              AND block_timestamp < ?
            "#,
        )
        .bind(delegator)
        .bind(orch)
        .bind(before)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get(0))
    }

    // ---- delegator_history -------------------------------------------------

    /// Returns `true` if this is the first time we've recorded
    /// `(delegator, orch)`. Side-effect: persists the row when first seen.
    pub async fn record_first_seen(
        &self,
        delegator_address: &str,
        orch_address: &str,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO delegator_history
                (delegator_address, orch_address, first_seen_at)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(delegator_address)
        .bind(orch_address)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
