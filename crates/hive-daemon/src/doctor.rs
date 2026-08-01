//! `hive-daemon --doctor`: runs each init step non-fatally and prints
//! `[OK]`/`[FAIL]` for one, so a first-run failure can be diagnosed from a
//! console even when the normal startup path dies silently under a GUI shell.

use abigail_core::vault::unlock::{HybridUnlockProvider, UnlockProvider};
use abigail_core::{AppConfig, SecretsVault};
use abigail_identity::IdentityManager;
use std::path::Path;

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn check(name: &'static str, result: anyhow::Result<String>) -> Check {
    match result {
        Ok(detail) => Check {
            name,
            ok: true,
            detail,
        },
        Err(e) => Check {
            name,
            ok: false,
            detail: format!("{e:#}"),
        },
    }
}

/// Run all checks and print a report. Returns the process exit code
/// (0 if every check passed, 1 otherwise).
pub fn run(data_dir: Option<&str>, port: u16) -> i32 {
    let mut checks = Vec::new();

    checks.push(check("log dir writable", check_log_dir()));

    let data_root = if let Some(dir) = data_dir {
        std::path::PathBuf::from(dir)
    } else {
        AppConfig::default_paths().data_dir
    };
    checks.push(check("data dir writable", check_data_dir(&data_root)));

    checks.push(check("port bind", check_port_bind(port)));

    abigail_core::vault::unlock::configure_process_vault_data_dir(&data_root);
    checks.push(check("vault KEK", check_vault_kek()));

    checks.push(check("identity", check_identity(&data_root)));
    checks.push(check("secrets", check_secrets(&data_root)));
    checks.push(check("entity-daemon binary", check_entity_daemon_binary()));

    println!("hive-daemon --doctor");
    println!("  data dir: {}", data_root.display());
    println!("  log dir:  {}", abigail_diag::log_dir().display());
    println!();

    let mut all_ok = true;
    for c in &checks {
        all_ok &= c.ok;
        let status = if c.ok { "OK" } else { "FAIL" };
        println!("[{status}] {}: {}", c.name, c.detail);
    }

    if all_ok {
        0
    } else {
        1
    }
}

fn check_log_dir() -> anyhow::Result<String> {
    let dir = abigail_diag::log_dir();
    std::fs::create_dir_all(&dir)?;
    let probe = dir.join(".doctor-probe");
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)?;
    Ok(dir.display().to_string())
}

fn check_data_dir(data_root: &Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(data_root)?;
    let probe = data_root.join(".doctor-probe");
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)?;
    Ok(data_root.display().to_string())
}

fn check_port_bind(port: u16) -> anyhow::Result<String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(format!("bound {} then released", addr))
}

fn check_vault_kek() -> anyhow::Result<String> {
    HybridUnlockProvider::new().root_kek()?;
    Ok("root KEK acquired".to_string())
}

fn check_identity(data_root: &Path) -> anyhow::Result<String> {
    let manager = IdentityManager::new(data_root.to_path_buf())?;
    match manager.hive_agent_id() {
        Ok(id) => Ok(format!("Abigail Hive identity resolved ({id})")),
        Err(e) => anyhow::bail!("identity manager initialized but hive agent unresolved: {e}"),
    }
}

fn check_secrets(data_root: &Path) -> anyhow::Result<String> {
    let hive_secrets_dir = data_root.join("hive_secrets");
    std::fs::create_dir_all(&hive_secrets_dir)?;
    if hive_secrets_dir.join("secrets.bin").exists() {
        SecretsVault::load(hive_secrets_dir)?;
    } else {
        SecretsVault::new(hive_secrets_dir);
    }
    Ok("hive secrets vault opened".to_string())
}

fn check_entity_daemon_binary() -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("current executable has no parent directory"))?;
    let name = if cfg!(windows) {
        "entity-daemon.exe"
    } else {
        "entity-daemon"
    };
    let path = dir.join(name);
    if path.is_file() {
        Ok(path.display().to_string())
    } else {
        anyhow::bail!("not found next to hive-daemon at {}", path.display())
    }
}
