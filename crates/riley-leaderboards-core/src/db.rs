use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};

use crate::config::DatabaseConfig;
use crate::error::{Error, Result};

/// Connect to Postgres with optional schema isolation.
///
/// When `schema` is set to something other than "public", this:
/// 1. Creates the schema if it doesn't exist (via a one-off connection)
/// 2. Builds the real pool with `search_path` set to `{schema}, public`
///    on every connection (so extensions like uuidv7() installed in `public`
///    remain accessible)
///
/// The schema is created *before* the pool so that `after_connect` hooks
/// never reference a non-existent schema.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool> {
    let url = config.url.resolve()?;
    let schema = config
        .schema
        .clone()
        .unwrap_or_else(|| "public".to_string());

    // Ensure the schema exists before building the pool. A one-off connection
    // guarantees the schema is there before any pooled connection's
    // after_connect hook fires.
    if schema != "public" {
        let bootstrap = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await?;
        sqlx::query(&format!(
            "CREATE SCHEMA IF NOT EXISTS {}",
            quote_identifier(&schema)
        ))
        .execute(&bootstrap)
        .await?;
        bootstrap.close().await;
    }

    let pool = build_pool(&url, &schema, config.max_connections).await?;
    Ok(pool)
}

/// Connect to Postgres in read-only mode (no schema creation side effects).
///
/// Used by commands like `validate` that should not mutate database state.
/// Verifies the configured schema exists before returning.
pub async fn connect_readonly(config: &DatabaseConfig) -> Result<PgPool> {
    let url = config.url.resolve()?;
    let schema = config
        .schema
        .clone()
        .unwrap_or_else(|| "public".to_string());

    let pool = build_pool(&url, &schema, config.max_connections).await?;

    // Verify the schema actually exists (search_path silently accepts nonexistent schemas)
    if schema != "public" {
        let exists: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
        )
        .bind(&schema)
        .fetch_one(&pool)
        .await?;
        if !exists.0 {
            pool.close().await;
            return Err(Error::Config(format!(
                "configured schema '{schema}' does not exist"
            )));
        }
    }

    Ok(pool)
}

async fn build_pool(url: &str, schema: &str, max_connections: u32) -> Result<PgPool> {
    let mut pool_opts = PgPoolOptions::new().max_connections(max_connections);

    if schema != "public" {
        let schema_clone = schema.to_owned();
        pool_opts = pool_opts.after_connect(move |conn, _meta| {
            let schema = schema_clone.clone();
            Box::pin(async move {
                conn.execute(
                    format!("SET search_path TO {}, public", quote_identifier(&schema)).as_str(),
                )
                .await?;
                Ok(())
            })
        });
    }

    let pool = pool_opts.connect(url).await?;
    Ok(pool)
}

/// Run pending migrations against the connected database.
///
/// Migrations run in whatever schema the connection's `search_path` is set to,
/// so the `_sqlx_migrations` table and all created tables land in the correct
/// schema automatically.
pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}

/// Quote a SQL identifier to prevent injection.
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_identifier_simple() {
        assert_eq!(quote_identifier("leaderboards"), "\"leaderboards\"");
    }

    #[test]
    fn quote_identifier_with_quotes() {
        assert_eq!(quote_identifier("my\"schema"), "\"my\"\"schema\"");
    }
}
