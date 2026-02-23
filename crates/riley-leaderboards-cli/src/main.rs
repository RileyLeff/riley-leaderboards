use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use riley_leaderboards_api::AppState;
use riley_leaderboards_core::{config, db};

#[derive(Parser)]
#[command(name = "riley-leaderboards")]
#[command(about = "A general-purpose versioned ranking service")]
struct Cli {
    /// Path to config file (overrides search)
    #[arg(long, short)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the HTTP server
    Serve,
    /// Run database migrations
    Migrate,
    /// Check config and database connectivity
    Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    let config = config::load_config(cli.config.as_deref())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    match cli.command {
        Command::Serve => {
            let pool = db::connect(&config.database).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations complete");

            let state = Arc::new(AppState { pool, config });
            riley_leaderboards_api::serve(state).await?;
        }
        Command::Migrate => {
            let pool = db::connect(&config.database).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations complete");
        }
        Command::Validate => {
            tracing::info!("config loaded successfully");
            let pool = db::connect(&config.database).await?;
            sqlx::query("SELECT 1")
                .execute(&pool)
                .await
                .map_err(|e| anyhow::anyhow!("database connection failed: {e}"))?;
            tracing::info!("database connection successful");

            if let Some(schema) = &config.database.schema {
                tracing::info!("database schema: {schema}");
            }
        }
    }

    Ok(())
}
