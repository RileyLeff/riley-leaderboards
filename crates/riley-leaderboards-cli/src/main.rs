use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
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
    /// Sync boards from a directory of TOML files
    Sync {
        /// Path to boards directory (defaults to [sync] repo_path from config)
        path: Option<PathBuf>,
        /// Version note for any boards updated during sync
        #[arg(long)]
        note: Option<String>,
    },
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
        .context("failed to load config")?;

    match cli.command {
        Command::Serve => {
            let pool = db::connect(&config.database).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations complete");

            let auth_mode =
                riley_leaderboards_api::auth::AuthMode::from_config(config.auth.as_ref())
                    .await
                    .context("failed to initialize auth")?;

            let state = Arc::new(AppState {
                pool,
                config,
                auth_mode,
            });
            riley_leaderboards_api::serve(state).await?;
        }
        Command::Migrate => {
            let pool = db::connect(&config.database).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations complete");
        }
        Command::Validate => {
            tracing::info!("config loaded successfully");
            let pool = db::connect_readonly(&config.database).await?;
            sqlx::query("SELECT 1")
                .execute(&pool)
                .await
                .context("database connection failed")?;
            tracing::info!("database connection successful");

            if let Some(schema) = &config.database.schema {
                tracing::info!("database schema: {schema}");
            }
        }
        Command::Sync { path, note } => {
            let dir = path
                .or_else(|| {
                    config
                        .sync
                        .as_ref()
                        .and_then(|s| s.repo_path.as_ref())
                        .map(PathBuf::from)
                })
                .context("no path provided and no [sync] repo_path in config")?;

            let pool = db::connect(&config.database).await?;
            db::migrate(&pool).await?;

            let results =
                riley_leaderboards_core::sync::execute::sync_dir(&pool, &dir, note.as_deref())
                    .await
                    .context("sync failed")?;

            for result in &results {
                match &result.action {
                    riley_leaderboards_core::sync::execute::SyncAction::Created {
                        version_number,
                    } => {
                        tracing::info!(
                            "board '{}': created (version {version_number})",
                            result.slug
                        );
                    }
                    riley_leaderboards_core::sync::execute::SyncAction::Updated {
                        version_number,
                    } => {
                        tracing::info!(
                            "board '{}': updated (version {version_number})",
                            result.slug
                        );
                    }
                    riley_leaderboards_core::sync::execute::SyncAction::NoChange => {
                        tracing::info!("board '{}': no changes", result.slug);
                    }
                    riley_leaderboards_core::sync::execute::SyncAction::Skipped { reason } => {
                        tracing::warn!("board '{}': skipped — {reason}", result.slug);
                    }
                    riley_leaderboards_core::sync::execute::SyncAction::Failed { error } => {
                        tracing::error!("board '{}': FAILED — {error}", result.slug);
                    }
                }
            }
        }
    }

    Ok(())
}
