use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone)]
pub struct Subscription {
    pub discord_user_id: String,
    pub orchestrator_address: String,
    pub created_at: DateTime<Utc>,
    pub dm_failure_count: i64,
}

/// CRUD for the `subscriptions` table.
///
/// All writes are idempotent — `insert` upserts and `delete` is a no-op when
/// the row is missing. The repo does NOT enforce the per-user subscription
/// cap; that's the command handler's responsibility (it calls
/// `count_for_user` first).
#[derive(Debug)]
pub struct SqliteSubscriptionsRepo {
    pool: SqlitePool,
}

impl SqliteSubscriptionsRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Inserts or returns `false` if the (user, orch) pair already exists.
    /// Returns `true` when a new row was created.
    pub async fn insert(
        &self,
        discord_user_id: &str,
        orchestrator_address: &str,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO subscriptions
                (discord_user_id, orchestrator_address, created_at, dm_failure_count)
            VALUES (?, ?, ?, 0)
            "#,
        )
        .bind(discord_user_id)
        .bind(orchestrator_address)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Removes the (user, orch) pair. Returns `true` if a row was actually
    /// deleted, `false` if no subscription existed.
    pub async fn delete(
        &self,
        discord_user_id: &str,
        orchestrator_address: &str,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "DELETE FROM subscriptions WHERE discord_user_id = ? AND orchestrator_address = ?",
        )
        .bind(discord_user_id)
        .bind(orchestrator_address)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_for_user(&self, discord_user_id: &str) -> anyhow::Result<Vec<Subscription>> {
        let rows = sqlx::query(
            r#"
            SELECT discord_user_id, orchestrator_address, created_at, dm_failure_count
            FROM subscriptions
            WHERE discord_user_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(discord_user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Subscription {
                discord_user_id: r.get(0),
                orchestrator_address: r.get(1),
                created_at: r.get(2),
                dm_failure_count: r.get(3),
            })
            .collect())
    }

    pub async fn count_for_user(&self, discord_user_id: &str) -> anyhow::Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) FROM subscriptions WHERE discord_user_id = ?")
            .bind(discord_user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get(0))
    }

    /// Returns every user subscribed to a given orchestrator. Used by the
    /// reward poller (004b) and the subscriber digest poster (004c) for
    /// fan-out.
    pub async fn find_for_orchestrator(
        &self,
        orchestrator_address: &str,
    ) -> anyhow::Result<Vec<Subscription>> {
        let rows = sqlx::query(
            r#"
            SELECT discord_user_id, orchestrator_address, created_at, dm_failure_count
            FROM subscriptions
            WHERE orchestrator_address = ?
            "#,
        )
        .bind(orchestrator_address)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Subscription {
                discord_user_id: r.get(0),
                orchestrator_address: r.get(1),
                created_at: r.get(2),
                dm_failure_count: r.get(3),
            })
            .collect())
    }

    /// Increment the DM failure counter for a subscription. Returns the new
    /// counter value so the caller can decide whether to auto-unsubscribe.
    pub async fn increment_dm_failure(
        &self,
        discord_user_id: &str,
        orchestrator_address: &str,
    ) -> anyhow::Result<i64> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET dm_failure_count = dm_failure_count + 1
            WHERE discord_user_id = ? AND orchestrator_address = ?
            "#,
        )
        .bind(discord_user_id)
        .bind(orchestrator_address)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT dm_failure_count FROM subscriptions WHERE discord_user_id = ? AND orchestrator_address = ?",
        )
        .bind(discord_user_id)
        .bind(orchestrator_address)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.map(|r| r.get::<i64, _>(0)).unwrap_or(0))
    }

    /// Reset the failure counter to zero after a successful DM.
    pub async fn clear_dm_failure(
        &self,
        discord_user_id: &str,
        orchestrator_address: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET dm_failure_count = 0
            WHERE discord_user_id = ? AND orchestrator_address = ?
              AND dm_failure_count > 0
            "#,
        )
        .bind(discord_user_id)
        .bind(orchestrator_address)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
