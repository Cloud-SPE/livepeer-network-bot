//! Repo for the `reward_watch_state` table (migration 0007): per-(round,
//! orchestrator) ladder-alert progress for the reward-call watcher.

use chrono::Utc;
use sqlx::{Row, SqlitePool};

#[derive(Debug)]
pub struct RewardWatchRepo {
    pool: SqlitePool,
}

impl RewardWatchRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Ladder DMs already sent for `(round, orch)`. Zero when no row exists.
    pub async fn alerts_sent(&self, round: i64, orch_address: &str) -> anyhow::Result<i64> {
        let row = sqlx::query(
            "SELECT alerts_sent FROM reward_watch_state WHERE round = ? AND orch_address = ?",
        )
        .bind(round)
        .bind(orch_address)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<i64, _>(0)).unwrap_or(0))
    }

    /// Record that the ladder has advanced to `alerts_sent` rungs for
    /// `(round, orch)`. Upserts so the first alert creates the row.
    pub async fn set_alerts_sent(
        &self,
        round: i64,
        orch_address: &str,
        alerts_sent: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO reward_watch_state (round, orch_address, alerts_sent, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (round, orch_address)
            DO UPDATE SET alerts_sent = excluded.alerts_sent, updated_at = excluded.updated_at
            "#,
        )
        .bind(round)
        .bind(orch_address)
        .bind(alerts_sent)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark that a reward call was observed for `(round, orch)`. Only rows
    /// that were previously alerted exist, so this upserts too — a row with
    /// `resolved_at` set and `alerts_sent = 0` records "rewarded before the
    /// first ladder rung."
    pub async fn mark_resolved(&self, round: i64, orch_address: &str) -> anyhow::Result<()> {
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO reward_watch_state (round, orch_address, alerts_sent, resolved_at, updated_at)
            VALUES (?, ?, 0, ?, ?)
            ON CONFLICT (round, orch_address)
            DO UPDATE SET resolved_at = COALESCE(reward_watch_state.resolved_at, excluded.resolved_at),
                          updated_at = excluded.updated_at
            "#,
        )
        .bind(round)
        .bind(orch_address)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn was_missed_notified(
        &self,
        round: i64,
        orch_address: &str,
    ) -> anyhow::Result<bool> {
        let row = sqlx::query(
            r#"
            SELECT 1 FROM reward_watch_state
            WHERE round = ? AND orch_address = ? AND missed_notified_at IS NOT NULL
            "#,
        )
        .bind(round)
        .bind(orch_address)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub async fn mark_missed_notified(&self, round: i64, orch_address: &str) -> anyhow::Result<()> {
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO reward_watch_state (round, orch_address, alerts_sent, missed_notified_at, updated_at)
            VALUES (?, ?, 0, ?, ?)
            ON CONFLICT (round, orch_address)
            DO UPDATE SET missed_notified_at = COALESCE(reward_watch_state.missed_notified_at, excluded.missed_notified_at),
                          updated_at = excluded.updated_at
            "#,
        )
        .bind(round)
        .bind(orch_address)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Drop rows from rounds older than `before_round`. Watch state is only
    /// consulted for the current and previous round, so anything older is
    /// dead weight.
    pub async fn prune_before(&self, before_round: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM reward_watch_state WHERE round < ?")
            .bind(before_round)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
