//! CLI subcommand handlers.
//!
//! Each function loads AppConfig from the standard data directory,
//! opens the SecretsVault, and performs operations directly.

use abigail_core::{ops::is_reserved_provider_key, AppConfig, SecretsVault};
use abigail_runtime::validate_secret_namespace_from_manifests;

/// Load AppConfig from the default data directory.
fn load_config() -> anyhow::Result<AppConfig> {
    let defaults = AppConfig::default_paths();
    let config_path = defaults.config_path();
    if config_path.exists() {
        AppConfig::load(&config_path)
    } else {
        Ok(defaults)
    }
}

/// Load the SecretsVault from the config's data directory.
fn load_vault(config: &AppConfig) -> anyhow::Result<SecretsVault> {
    SecretsVault::load(config.data_dir.clone()).map_err(Into::into)
}

fn load_skills_vault(config: &AppConfig) -> anyhow::Result<SecretsVault> {
    SecretsVault::load_custom(config.data_dir.clone(), "skills.bin").map_err(Into::into)
}

pub fn status() -> anyhow::Result<()> {
    let config = load_config()?;
    println!("=== Abigail Agent Status ===");
    println!("Data directory: {}", config.data_dir.display());
    println!("Birth complete: {}", config.birth_complete);
    println!(
        "Agent name: {}",
        config.agent_name.as_deref().unwrap_or("(not set)")
    );
    println!("Routing mode: {:?}", config.routing_mode);

    if let Some(ref trinity) = config.trinity {
        println!(
            "Ego provider: {}",
            trinity.ego_provider.as_deref().unwrap_or("(none)")
        );
        println!(
            "Ego API key: {}",
            if trinity.ego_api_key.is_some() {
                "configured"
            } else {
                "not set"
            }
        );
        println!("Id URL: {}", trinity.id_url.as_deref().unwrap_or("(none)"));
    } else {
        println!("Trinity config: not configured");
    }

    println!(
        "Local LLM URL: {}",
        config.local_llm_base_url.as_deref().unwrap_or("(not set)")
    );

    println!("Email transport: removed from mainline Abigail");
    println!("Email accounts: 0 (deprecated compatibility field)");
    println!("MCP servers: {}", config.mcp_servers.len());
    println!("Approved skills: {}", config.approved_skill_ids.len());

    // Secrets vault summary
    match load_vault(&config) {
        Ok(vault) => {
            println!(
                "Secrets vault: {} keys stored",
                vault.list_providers().len()
            );
        }
        Err(e) => println!("Secrets vault: error loading — {}", e),
    }

    Ok(())
}

pub fn store_secret(key: &str, value: &str) -> anyhow::Result<()> {
    let config = load_config()?;
    validate_secret_namespace_from_manifests(&[], &[config.data_dir.join("skills")], key)
        .map_err(anyhow::Error::msg)?;
    let mut vault = if is_reserved_provider_key(key) {
        load_vault(&config)?
    } else {
        load_skills_vault(&config)?
    };
    abigail_core::ops::store_vault_secret(&mut vault, key, value)?;
    println!("Secret '{}' stored successfully.", key);
    Ok(())
}

pub fn check_secret(key: &str) -> anyhow::Result<()> {
    let config = load_config()?;
    let provider = load_vault(&config)?;
    let skills = load_skills_vault(&config)?;
    if abigail_core::ops::check_vault_secret(&provider, key)
        || abigail_core::ops::check_vault_secret(&skills, key)
    {
        println!("Secret '{}': EXISTS", key);
    } else {
        println!("Secret '{}': NOT FOUND", key);
    }
    Ok(())
}

pub fn list_secrets() -> anyhow::Result<()> {
    let config = load_config()?;
    let vault = load_vault(&config)?;
    let skills_vault = load_skills_vault(&config)?;
    let mut providers = vault.list_providers();
    providers.extend(skills_vault.list_providers());
    providers.sort();
    providers.dedup();
    if providers.is_empty() {
        println!("No secrets stored.");
    } else {
        println!("Stored secret keys:");
        for p in providers {
            println!("  - {}", p);
        }
    }
    Ok(())
}

pub fn configure_email(
    address: &str,
    imap_host: &str,
    imap_port: u16,
    smtp_host: &str,
    smtp_port: u16,
    password: &str,
) -> anyhow::Result<()> {
    let mut config = load_config()?;
    abigail_core::ops::set_email_config(
        &mut config,
        address.to_string(),
        imap_host.to_string(),
        imap_port,
        smtp_host.to_string(),
        smtp_port,
        password,
    )?;
    Ok(())
}

pub fn integration_status() -> anyhow::Result<()> {
    let config = load_config()?;
    let vault = load_vault(&config)?;
    let skills_vault = load_skills_vault(&config)?;
    let integrations = abigail_skills::preloaded_integration_skills();

    println!("=== Integration Status ===");
    if integrations.is_empty() {
        println!("No preloaded integrations found.");
        return Ok(());
    }

    for (skill_config, auth) in &integrations {
        let secret_keys = abigail_skills::dynamic::extract_secret_keys(skill_config);
        let missing: Vec<&str> = secret_keys
            .iter()
            .filter(|k| vault.get_secret(k).is_none() && skills_vault.get_secret(k).is_none())
            .map(|s| s.as_str())
            .collect();

        if missing.is_empty() {
            println!("  [OK] {} ({})", skill_config.name, auth.service_id);
        } else {
            println!(
                "  [!!] {} ({}) — missing: {}",
                skill_config.name,
                auth.service_id,
                missing.join(", ")
            );
            println!("       Setup: {}", auth.setup_url);
        }
    }
    Ok(())
}

pub fn router_status() -> anyhow::Result<()> {
    let config = load_config()?;
    println!("=== Router Status ===");
    println!("Routing mode: {:?}", config.routing_mode);

    if let Some(ref trinity) = config.trinity {
        println!(
            "Id (local): {}",
            trinity.id_url.as_deref().unwrap_or("CandleProvider stub")
        );
        println!(
            "Ego (cloud): {} {}",
            trinity.ego_provider.as_deref().unwrap_or("not configured"),
            if trinity.ego_api_key.is_some() {
                "(key set)"
            } else {
                "(no key)"
            }
        );
    } else {
        println!("Trinity: not configured (default Id-only mode)");
    }

    println!(
        "Local LLM URL: {}",
        config
            .local_llm_base_url
            .as_deref()
            .unwrap_or("not configured")
    );

    Ok(())
}
