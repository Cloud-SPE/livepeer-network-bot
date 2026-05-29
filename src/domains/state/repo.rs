use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{Row, SqlitePool};

use crate::domains::explorer::types::{preferred_valuation, Cadence, EventRow};

#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: String,
    pub tx_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub amount_native: Option<String>,
    pub amount_usd: Option<String>,
    pub native_usd_price: Option<String>,
}

/// Snapshot of a payout-summary rollup as last observed from the explorer.
/// Two consecutive snapshots with identical figures mean the rollup has
/// stabilized — see `summary_poster`'s readiness gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummarySnapshot {
    pub ticket_count: String,
    pub usd_rows_priced: String,
    pub sum_face_value_native: String,
    pub sum_commission_native: String,
}

#[derive(Debug, Clone, Copy)]
pub struct OrchTotals {
    pub face_value_eth: f64,
    pub face_value_usd: f64,
    pub commission_eth: f64,
    pub commission_usd: f64,
}

pub struct SqliteStateRepo {
    pool: SqlitePool,
}

impl SqliteStateRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert_event(&self, ev: &EventRow) -> anyhow::Result<bool> {
        let val = ev.valuations.as_ref().and_then(|v| preferred_valuation(v));
        let amount_usd = val.and_then(|v| v.amount_usd.clone());
        let native_usd_price = val.and_then(|v| v.native_usd_price.clone());

        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO events (
                id, chain_id, tx_hash, log_index, block_number, block_timestamp,
                contract_address, contract_name, event_name, event_signature,
                asset, amount_native, amount_usd, native_usd_price,
                from_address, to_address, finality, is_canonical, fetched_at
            ) VALUES (
                ?, ?, ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?, ?
            )
            "#,
        )
        .bind(&ev.id)
        .bind(&ev.chain_id)
        .bind(&ev.tx_hash)
        .bind(ev.log_index)
        .bind(&ev.block_number)
        .bind(ev.block_timestamp)
        .bind(&ev.contract_address)
        .bind(&ev.contract_name)
        .bind(&ev.event_name)
        .bind(&ev.event_signature)
        .bind(&ev.asset)
        .bind(&ev.amount_native)
        .bind(&amount_usd)
        .bind(&native_usd_price)
        .bind(&ev.from_address)
        .bind(&ev.to_address)
        .bind(&ev.finality)
        .bind(ev.is_canonical as i64)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        if amount_usd.is_some() || native_usd_price.is_some() {
            self.repair_pending_event_valuation(&ev.id, amount_usd, native_usd_price)
                .await?;
        }
        Ok(result.rows_affected() > 0)
    }

    pub async fn repair_pending_event_valuation(
        &self,
        id: &str,
        amount_usd: Option<String>,
        native_usd_price: Option<String>,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE events
               SET amount_usd = CASE
                       WHEN ? IS NOT NULL AND CAST(? AS REAL) > 0
                            AND (amount_usd IS NULL OR CAST(amount_usd AS REAL) <= 0)
                       THEN ?
                       ELSE amount_usd
                   END,
                   native_usd_price = CASE
                       WHEN ? IS NOT NULL AND CAST(? AS REAL) > 0
                            AND (native_usd_price IS NULL OR CAST(native_usd_price AS REAL) <= 0)
                       THEN ?
                       ELSE native_usd_price
                   END
             WHERE id = ?
               AND sent_to_discord = 0
            "#,
        )
        .bind(&amount_usd)
        .bind(&amount_usd)
        .bind(&amount_usd)
        .bind(&native_usd_price)
        .bind(&native_usd_price)
        .bind(&native_usd_price)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_cursor(&self, name: &str) -> anyhow::Result<Option<String>> {
        let row = sqlx::query("SELECT cursor_value FROM cursors WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>(0)))
    }

    pub async fn set_cursor(&self, name: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO cursors (name, cursor_value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(name) DO UPDATE SET
                cursor_value = excluded.cursor_value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(name)
        .bind(value)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fetch_unsent(&self, limit: i64) -> anyhow::Result<Vec<StoredEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tx_hash, block_timestamp, from_address, to_address,
                   amount_native, amount_usd, native_usd_price
            FROM events
            WHERE sent_to_discord = 0
              AND event_name = 'WinningTicketRedeemed'
            ORDER BY block_timestamp ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| StoredEvent {
                id: r.get(0),
                tx_hash: r.get(1),
                block_timestamp: r.get(2),
                from_address: r.get(3),
                to_address: r.get(4),
                amount_native: r.get(5),
                amount_usd: r.get(6),
                native_usd_price: r.get(7),
            })
            .collect())
    }

    pub async fn mark_sent(&self, ids: &[String]) -> anyhow::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        let now = Utc::now();
        for id in ids {
            sqlx::query("UPDATE events SET sent_to_discord = 1, sent_at = ? WHERE id = ?")
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Sum face-value + commission for an orchestrator across the half-open
    /// window `(since, until]` (inclusive at both bounds — see SQL). Both
    /// bounds are required so that the rolling total stays meaningful when
    /// `digest_poster` is draining a backfill: anchor at the digest's
    /// latest ticket, not at wall-clock `now()`.
    pub async fn orch_totals_window(
        &self,
        to_address: &str,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
        fee_cut: f64,
    ) -> anyhow::Result<OrchTotals> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CAST(amount_native AS REAL)), 0.0) AS face_eth,
                COALESCE(SUM(CAST(amount_usd AS REAL)), 0.0) AS face_usd
            FROM events
            WHERE event_name = 'WinningTicketRedeemed'
              AND to_address = ?
              AND block_timestamp >= ?
              AND block_timestamp <= ?
            "#,
        )
        .bind(to_address)
        .bind(since)
        .bind(until)
        .fetch_one(&self.pool)
        .await?;

        let face_eth: f64 = row.get(0);
        let face_usd: f64 = row.get(1);
        Ok(OrchTotals {
            face_value_eth: face_eth,
            face_value_usd: face_usd,
            commission_eth: face_eth * fee_cut,
            commission_usd: face_usd * fee_cut,
        })
    }

    /// Count of locally-ingested WinningTicketRedeemed events in the
    /// half-open window `[since, until)`. The summary poster uses this as a
    /// cross-check: if the explorer's rollup reports fewer tickets than the
    /// bot has already seen on-chain, the rollup is still catching up.
    pub async fn count_winning_tickets_in_window(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> anyhow::Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*)
            FROM events
            WHERE event_name = 'WinningTicketRedeemed'
              AND block_timestamp >= ?
              AND block_timestamp < ?
            "#,
        )
        .bind(since)
        .bind(until)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get(0))
    }

    pub async fn get_summary_snapshot(
        &self,
        cadence: Cadence,
        period_date: NaiveDate,
    ) -> anyhow::Result<Option<SummarySnapshot>> {
        let row = sqlx::query(
            r#"
            SELECT ticket_count, usd_rows_priced, sum_face_value_native, sum_commission_native
            FROM summary_snapshots
            WHERE period = ? AND period_date = ?
            "#,
        )
        .bind(cadence.as_path())
        .bind(period_date.format("%Y-%m-%d").to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| SummarySnapshot {
            ticket_count: r.get(0),
            usd_rows_priced: r.get(1),
            sum_face_value_native: r.get(2),
            sum_commission_native: r.get(3),
        }))
    }

    pub async fn upsert_summary_snapshot(
        &self,
        cadence: Cadence,
        period_date: NaiveDate,
        snap: &SummarySnapshot,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO summary_snapshots (
                period, period_date, ticket_count, usd_rows_priced,
                sum_face_value_native, sum_commission_native, observed_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(period, period_date) DO UPDATE SET
                ticket_count = excluded.ticket_count,
                usd_rows_priced = excluded.usd_rows_priced,
                sum_face_value_native = excluded.sum_face_value_native,
                sum_commission_native = excluded.sum_commission_native,
                observed_at = excluded.observed_at
            "#,
        )
        .bind(cadence.as_path())
        .bind(period_date.format("%Y-%m-%d").to_string())
        .bind(&snap.ticket_count)
        .bind(&snap.usd_rows_priced)
        .bind(&snap.sum_face_value_native)
        .bind(&snap.sum_commission_native)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn summary_posted(
        &self,
        cadence: Cadence,
        period_date: NaiveDate,
    ) -> anyhow::Result<bool> {
        let row =
            sqlx::query("SELECT 1 FROM summary_watermarks WHERE period = ? AND period_date = ?")
                .bind(cadence.as_path())
                .bind(period_date.format("%Y-%m-%d").to_string())
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.is_some())
    }

    pub async fn mark_summary_posted(
        &self,
        cadence: Cadence,
        period_date: NaiveDate,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO summary_watermarks (period, period_date, posted_at)
            VALUES (?, ?, ?)
            ON CONFLICT(period, period_date) DO NOTHING
            "#,
        )
        .bind(cadence.as_path())
        .bind(period_date.format("%Y-%m-%d").to_string())
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
