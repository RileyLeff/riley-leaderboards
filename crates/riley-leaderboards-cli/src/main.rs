use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use riley_leaderboards_api::AppState;
use riley_leaderboards_core::{config, db, repo};
use tokio::task::JoinHandle;

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
    /// List all boards
    ListBoards,
    /// Delete a board and all its data
    DeleteBoard {
        /// Board slug to delete
        slug: String,
    },
    /// List versions for a board
    ListVersions {
        /// Board slug
        slug: String,
    },
    /// Export a board as JSON (all versions with placements)
    Export {
        /// Board slug to export
        slug: String,
    },
    /// Import a board from a JSON export file
    Import {
        /// Path to JSON export file
        file: PathBuf,
    },
    /// List all collections
    ListCollections,
    /// Delete a collection (does not delete its boards)
    DeleteCollection {
        /// Collection slug to delete
        slug: String,
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

            let redis = if let Some(ref redis_config) = config.redis {
                let url = redis_config.url.resolve().context("failed to resolve Redis URL")?;
                let client = redis::Client::open(url.as_str())
                    .context("failed to create Redis client")?;
                let mgr = redis::aio::ConnectionManager::new(client)
                    .await
                    .context("failed to connect to Redis")?;
                tracing::info!("connected to Redis");
                Some(mgr)
            } else {
                None
            };

            let server_config = config.server.as_ref().cloned().unwrap_or_default();
            let event_bus = if server_config.sse_enabled {
                tracing::info!(
                    "SSE enabled (max_connections={}, debounce_ms={})",
                    server_config.sse_max_connections,
                    server_config.sse_score_debounce_ms
                );
                Some(riley_leaderboards_api::sse::EventBus::new(
                    server_config.sse_max_connections,
                    server_config.sse_score_debounce_ms,
                    server_config.sse_broadcast_buffer,
                ))
            } else {
                None
            };

            let state = Arc::new(AppState {
                pool,
                redis,
                config,
                auth_mode,
                sync_mutex: tokio::sync::Mutex::new(()),
                event_bus,
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

            let mut webhook_handles: Vec<JoinHandle<()>> = Vec::new();
            for result in &results {
                match &result.action {
                    riley_leaderboards_core::sync::execute::SyncAction::Created {
                        version_number,
                    } => {
                        tracing::info!(
                            "board '{}': created (version {version_number})",
                            result.slug
                        );
                        // Fire board.created for newly created boards
                        webhook_handles.extend(riley_leaderboards_api::outbound_webhooks::fire(
                            &config.webhooks,
                            riley_leaderboards_core::config::WebhookEvent::BoardCreated,
                            &result.slug,
                            &result.name,
                            None,
                            None,
                        ));
                        webhook_handles.extend(riley_leaderboards_api::outbound_webhooks::fire(
                            &config.webhooks,
                            riley_leaderboards_core::config::WebhookEvent::VersionCreated,
                            &result.slug,
                            &result.name,
                            Some(riley_leaderboards_api::outbound_webhooks::VersionInfo {
                                version_number: *version_number,
                                note: note.clone(),
                            }),
                            None,
                        ));
                    }
                    riley_leaderboards_core::sync::execute::SyncAction::Updated {
                        version_number,
                    } => {
                        tracing::info!(
                            "board '{}': updated (version {version_number})",
                            result.slug
                        );
                        webhook_handles.extend(riley_leaderboards_api::outbound_webhooks::fire(
                            &config.webhooks,
                            riley_leaderboards_core::config::WebhookEvent::VersionCreated,
                            &result.slug,
                            &result.name,
                            Some(riley_leaderboards_api::outbound_webhooks::VersionInfo {
                                version_number: *version_number,
                                note: note.clone(),
                            }),
                            None,
                        ));
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
            // Await webhook delivery before CLI exits
            futures_util::future::join_all(webhook_handles).await;
        }
        Command::ListBoards => {
            let pool = db::connect_readonly(&config.database).await?;
            let boards = repo::boards::list(&pool).await?;
            if boards.is_empty() {
                println!("No boards found.");
            } else {
                println!("{:<20} {:<10} {:<6} {:<6} {}", "SLUG", "TYPE", "SORT", "ACCUM", "NAME");
                for b in &boards {
                    println!(
                        "{:<20} {:<10} {:<6} {:<6} {}",
                        b.slug, b.board_type, b.sort_direction,
                        if b.accumulative { "yes" } else { "no" },
                        b.name
                    );
                }
                println!("\n{} board(s)", boards.len());
            }
        }
        Command::DeleteBoard { slug } => {
            let pool = db::connect(&config.database).await?;
            let board = repo::boards::get_by_slug(&pool, &slug).await
                .context("board not found")?;
            let board_name = board.name.clone();

            // Clean up Redis keys for realtime boards before Postgres delete
            if board.realtime {
                if let Some(ref redis_config) = config.redis {
                    let url = redis_config.url.resolve().context("failed to resolve Redis URL")?;
                    let client = redis::Client::open(url.as_str())
                        .context("failed to create Redis client")?;
                    let mut mgr = redis::aio::ConnectionManager::new(client)
                        .await
                        .context("failed to connect to Redis")?;
                    let prefix = config.redis_key_prefix();
                    let _ = repo::realtime::clear(&mut mgr, &slug, prefix).await;
                }
            }

            repo::boards::delete(&pool, &slug).await?;
            let handles = riley_leaderboards_api::outbound_webhooks::fire(
                &config.webhooks,
                riley_leaderboards_core::config::WebhookEvent::BoardDeleted,
                &slug,
                &board_name,
                None,
                None,
            );
            futures_util::future::join_all(handles).await;
            println!("Board '{slug}' deleted.");
        }
        Command::ListVersions { slug } => {
            let pool = db::connect_readonly(&config.database).await?;
            let board = repo::boards::get_by_slug(&pool, &slug).await?;
            let versions = repo::versions::list(&pool, board.id).await?;
            if versions.is_empty() {
                println!("No versions for board '{slug}'.");
            } else {
                println!("{:<8} {:<28} {}", "VERSION", "CREATED", "NOTE");
                for v in &versions {
                    println!(
                        "{:<8} {:<28} {}",
                        v.version_number,
                        v.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
                        v.note.as_deref().unwrap_or("-")
                    );
                }
                println!("\n{} version(s)", versions.len());
            }
        }
        Command::Export { slug } => {
            let pool = db::connect_readonly(&config.database).await?;
            let export = repo::export::export_board(&pool, &slug).await?;
            let json = serde_json::to_string_pretty(&export)
                .context("failed to serialize export")?;
            println!("{json}");
        }
        Command::Import { file } => {
            let contents = std::fs::read_to_string(&file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let export: repo::export::BoardExport = serde_json::from_str(&contents)
                .with_context(|| format!("failed to parse {}", file.display()))?;
            let pool = db::connect(&config.database).await?;
            db::migrate(&pool).await?;
            repo::export::import_board(&pool, &export).await?;
            let handles = riley_leaderboards_api::outbound_webhooks::fire(
                &config.webhooks,
                riley_leaderboards_core::config::WebhookEvent::BoardCreated,
                &export.board.slug,
                &export.board.name,
                None,
                None,
            );
            futures_util::future::join_all(handles).await;
            println!("Board '{}' imported successfully.", export.board.slug);
        }
        Command::ListCollections => {
            let pool = db::connect_readonly(&config.database).await?;
            let collections = repo::collections::list(&pool).await?;
            if collections.is_empty() {
                println!("No collections found.");
            } else {
                println!("{:<30} {}", "SLUG", "NAME");
                for c in &collections {
                    println!("{:<30} {}", c.slug, c.name);
                }
                println!("\n{} collection(s)", collections.len());
            }
        }
        Command::DeleteCollection { slug } => {
            let pool = db::connect(&config.database).await?;
            repo::collections::delete(&pool, &slug).await?;
            println!("Collection '{slug}' deleted.");
        }
    }

    Ok(())
}
