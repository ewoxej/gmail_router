use anyhow::Result;
use gmail_router::config::{get_config_path, CREDENTIALS_FILE, ROUTING_FILE};
use gmail_router::{config, gmail, processor};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "list-messages" => list_messages().await?,
        "check-config" => check_config()?,
        "test-auth" => test_auth().await?,
        "count-addresses" => count_addresses().await?,
        _ => {
            println!("Unknown command: {}", args[1]);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Gmail Router - test utility\n");
    println!("Usage: cargo run --bin test_util -- [команда]\n");
    println!("Commands:");
    println!("  list-messages    - List all messages");
    println!("  check-config     - Check configuration files");
    println!("  test-auth        - Check Gmail API authentification");
    println!("  count-addresses  - Count unique addresses");
}

async fn list_messages() -> Result<()> {
    println!("Loading messages...\n");

    let creds_config = config::CredentialsConfig::load(get_config_path(CREDENTIALS_FILE))?;
    let gmail_client = gmail::GmailClient::new(&creds_config.google_credentials_path).await?;

    let date_filter = creds_config.start_date.format("%Y/%m/%d").to_string();
    let message_ids = gmail_client.list_messages(&date_filter).await?;

    println!("Messages found: {}\n", message_ids.len());

    if !message_ids.is_empty() {
        println!("First 10 messages:");
        for (i, id) in message_ids.iter().take(10).enumerate() {
            let msg = gmail_client.get_message(id).await?;
            let subject = msg
                .payload
                .as_ref()
                .and_then(|p| p.headers.as_ref())
                .and_then(|headers| {
                    headers
                        .iter()
                        .find(|h| h.name.as_ref().map(|n| n == "Subject").unwrap_or(false))
                        .and_then(|h| h.value.clone())
                })
                .unwrap_or_else(|| "(no subject)".to_string());

            println!("  {}. {}", i + 1, subject);
        }
    }

    Ok(())
}

fn check_config() -> Result<()> {
    println!("Checking config...\n");

    print!("Checking credentials.yaml... ");
    match config::CredentialsConfig::load(get_config_path(CREDENTIALS_FILE)) {
        Ok(config) => {
            println!("  Domain: {}", config.domain);
            println!("  Check interval: {} s", config.check_interval_seconds);
            println!("  Start date: {}", config.start_date);
        }
        Err(e) => {
            println!("  Error: {}", e);
            return Err(e);
        }
    }

    println!();

    print!("Checking routing.yaml... ");
    match config::RoutingConfig::load(get_config_path(ROUTING_FILE)) {
        Ok(config) => {
            println!("  Addresses count: {}", config.addresses.len());

            let allowed = config.addresses.values().filter(|&&v| v).count();
            let blocked = config.addresses.values().filter(|&&v| !v).count();

            println!("  Allowed: {}", allowed);
            println!("  Banned: {}", blocked);

            if blocked > 0 {
                println!("\n  Banned addreses");
                for (addr, &enabled) in &config.addresses {
                    if !enabled {
                        println!("    - {}", addr);
                    }
                }
            }
        }
        Err(e) => {
            println!("  Error: {} (maybe file hasnt been created yet)", e);
        }
    }

    Ok(())
}

async fn test_auth() -> Result<()> {
    println!("Check Gmail API authentification...\n");

    let creds_config = config::CredentialsConfig::load(get_config_path(CREDENTIALS_FILE))?;

    print!("Creating Gmail client... ");
    match gmail::GmailClient::new(&creds_config.google_credentials_path).await {
        Ok(_) => {
            println!("\nAuth success!");
        }
        Err(e) => {
            println!("\nError: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

async fn count_addresses() -> Result<()> {
    println!("Count unique emails\n");

    let creds_config = config::CredentialsConfig::load(get_config_path(CREDENTIALS_FILE))?;
    let gmail_client = gmail::GmailClient::new(&creds_config.google_credentials_path).await?;

    let date_filter = creds_config.start_date.format("%Y/%m/%d").to_string();
    let message_ids = gmail_client.list_messages(&date_filter).await?;

    println!("Total: {}", message_ids.len());
    println!("Scan addreses...\n");

    let addresses =
        processor::collect_all_addresses(&gmail_client, &message_ids, &creds_config.domain).await?;

    println!("Unique emails found: {}\n", addresses.len());

    let mut sorted_addresses: Vec<_> = addresses.iter().collect();
    sorted_addresses.sort();

    println!("Address list:");
    for addr in sorted_addresses {
        println!("  - {}@{}", addr, creds_config.domain);
    }

    Ok(())
}
