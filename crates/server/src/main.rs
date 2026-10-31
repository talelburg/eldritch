//! Eldritch server binary: serves the HTTP/WS router, or with `accounts`,
//! edits the allowlist and exits.
//!
//! Both read their configuration from the environment and apply the
//! migrations to the `SQLite` database before doing anything else.

use std::env;
use std::error::Error;
use std::io;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use server::accounts::{self, Account, Email, EntryRef};
use server::auth::github::GitHub;
use server::auth::provider::IdentityProvider;
use server::config::{Config, ConfigError};
use server::db::{self, MIGRATOR};
use server::AppState;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

/// The Eldritch server. With no subcommand, it serves the game.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Serve the dev sign-in picker at /auth/dev, which signs in as any
    /// account on the allowlist. It answers only a loopback Host.
    #[arg(long)]
    dev_auth: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Keep the allowlist: who may sign in, and as which account.
    #[command(subcommand)]
    Accounts(AccountsCommand),
}

#[derive(Subcommand)]
enum AccountsCommand {
    /// Add an entry holding one or more emails.
    Add {
        /// The name shown at the table.
        #[arg(long)]
        name: String,
        /// Every email that signs in as this entry.
        #[arg(required = true)]
        emails: Vec<String>,
    },
    /// Disable an entry, keeping it for the games that refer to it, and end
    /// its sessions.
    Remove {
        /// One of the entry's emails, or its id.
        entry: String,
    },
    /// Print every entry: its id, name, emails, and whether it is enabled.
    List,
}

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        None => serve(cli.dev_auth).await.map(|()| ExitCode::SUCCESS),
        Some(Command::Accounts(command)) => Ok(run_accounts(command).await),
    }
}

/// The log filter: `RUST_LOG`, or `info` when it is unset.
fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())
}

/// The configuration, read from the process environment.
fn config() -> Result<Config, ConfigError> {
    Config::from_lookup(|name| env::var(name).ok())
}

/// Open the configured database and apply the migrations.
async fn open_database(config: &Config) -> Result<SqlitePool, Box<dyn Error>> {
    let pool = db::connect_pool(&config.database_url).await?;
    MIGRATOR.run(&pool).await?;
    tracing::info!("database ready at {}", config.database_url);
    Ok(pool)
}

async fn serve(dev_auth: bool) -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(env_filter())
        .init();

    // Logged before returning: `main`'s error exit prints the `Debug` form,
    // which doesn't name the variable.
    let config = config().inspect_err(|error| tracing::error!("{error}"))?;
    let pool = open_database(&config).await?;

    // A missing bundle only warns: the API and WebSocket still serve, which
    // is what a dev run without `trunk build` wants. On a deploy it means a
    // mistyped `ELDRITCH_DIST`, and this line is where the journal shows it.
    let index_html = config.dist_dir.join("index.html");
    if !index_html.is_file() {
        tracing::warn!(
            "no client bundle at {}; serving the API only",
            index_html.display()
        );
    }

    if dev_auth {
        tracing::warn!("--dev-auth is on: /auth/dev signs any loopback request in as any account");
    }

    let listener = TcpListener::bind(config.bind).await?;
    tracing::info!(
        "eldritch server (build {}) listening on http://{}",
        protocol::build_id(),
        config.bind
    );

    let providers = identity_providers(&config);
    let state =
        AppState::new_with_dist(pool, server::REGISTRIES, config.dist_dir).with_dev_auth(dev_auth);
    let state = match &config.public_origin {
        Some(origin) => state.with_identity_providers(origin, providers),
        None => state,
    };
    axum::serve(listener, server::app(state)).await?;
    Ok(())
}

/// The adapter of each provider `config` turns on, in the order the sign-in
/// screen lists them.
fn identity_providers(config: &Config) -> Vec<Arc<dyn IdentityProvider>> {
    let mut providers: Vec<Arc<dyn IdentityProvider>> = Vec::new();
    if let Some(credentials) = &config.github {
        providers.push(Arc::new(GitHub::new(credentials)));
    }
    for provider in &providers {
        tracing::info!("sign-in with {} is on", provider.name());
    }
    providers
}

/// Run one `accounts` subcommand. Its output goes to stdout and everything
/// else to stderr, logs included, so `list` can be read by a script.
async fn run_accounts(command: AccountsCommand) -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(env_filter())
        .with_writer(io::stderr)
        .init();

    match edit_allowlist(command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn edit_allowlist(command: AccountsCommand) -> Result<(), Box<dyn Error>> {
    let pool = open_database(&config()?).await?;
    match command {
        AccountsCommand::Add { name, emails } => {
            let account = accounts::add(&pool, &name, &emails).await?;
            println!("added {}", entry_line(&account));
        }
        AccountsCommand::Remove { entry } => {
            let id = accounts::remove(&pool, &EntryRef::parse(&entry)?).await?;
            println!("disabled entry {id}");
        }
        AccountsCommand::List => {
            for account in accounts::list(&pool).await? {
                println!("{}", entry_line(&account));
            }
        }
    }
    Ok(())
}

/// One entry as `list` prints it: id, name, emails and status, tab-separated.
fn entry_line(account: &Account) -> String {
    let status = if account.disabled {
        "disabled"
    } else {
        "enabled"
    };
    format!(
        "{}\t{}\t{}\t{status}",
        account.id,
        account.display_name,
        account
            .emails
            .iter()
            .map(Email::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    )
}
