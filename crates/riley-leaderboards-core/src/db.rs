use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};

use crate::config::DatabaseConfig;
use crate::error::Result;

/// Connect to Postgres with optional schema isolation.
///
/// When `schema` is set to something other than "public", the pool:
/// 1. Sets `search_path` to `{schema}, public` on every connection (so
///    extensions like uuidv7() installed in `public` remain accessible)
/// 2. Creates the schema if it doesn't exist
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool> {
    let url = config.url.resolve()?;
    let schema = config
        .schema
        .clone()
        .unwrap_or_else(|| "public".to_string());

    let mut pool_opts = PgPoolOptions::new().max_connections(config.max_connections);

    if schema != "public" {
        let schema_clone = schema.clone();
        pool_opts = pool_opts.after_connect(move |conn, _meta| {
            let schema = schema_clone.clone();
            Box::pin(async move {
                conn.execute(
                    format!(
                        "SET search_path TO {}, public",
                        quote_identifier(&schema)
                    )
                    .as_str(),
                )
                .await?;
                Ok(())
            })
        });
    }

    let pool = pool_opts.connect(&url).await?;

    if schema != "public" {
        sqlx::query(&format!(
            "CREATE SCHEMA IF NOT EXISTS {}",
            quote_identifier(&schema)
        ))
        .execute(&pool)
        .await?;
    }

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
