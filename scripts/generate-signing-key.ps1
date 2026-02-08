# Generate external signing keypair and sign constitutional documents.
# The private key should be stored securely OUTSIDE the Abigail repository.
# The public key is placed in a protected location for Abigail to read.
#
# Prerequisites: Rust toolchain (for the signing tool)
#
# Usage:
#   .\scripts\generate-signing-key.ps1 -OutputDir "C:\SecureKeys\abigail"
#
# This creates:
#   - $OutputDir\signing_key.bin (PRIVATE - keep secure, never commit)
#   - $OutputDir\pubkey.bin (PUBLIC - configure in Abigail's external_pubkey_path)
#   - templates\soul.md.sig, ethics.md.sig, instincts.md.sig (commit these)

param(
    [Parameter(Mandatory=$true)]
    [string]$OutputDir
)

$ErrorActionPreference = "Stop"

# Ensure output directory exists
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

Write-Host "Generating Ed25519 signing keypair..."
Write-Host "  Private key: $OutputDir\signing_key.bin"
Write-Host "  Public key:  $OutputDir\pubkey.bin"

# Build and run the signing tool
$toolDir = Join-Path $PSScriptRoot ".." "tools" "sign-docs"
if (-not (Test-Path $toolDir)) {
    Write-Host "Creating signing tool at $toolDir..."
    New-Item -ItemType Directory -Force -Path $toolDir | Out-Null
    
    # Create Cargo.toml for the signing tool
    @"
[package]
name = "sign-docs"
version = "0.1.0"
edition = "2021"

[dependencies]
ed25519-dalek = { version = "2.1", features = ["rand_core"] }
rand = "0.8"
base64 = "0.21"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
"@ | Out-File -FilePath (Join-Path $toolDir "Cargo.toml") -Encoding utf8

    # Create src/main.rs for the signing tool
    New-Item -ItemType Directory -Force -Path (Join-Path $toolDir "src") | Out-Null
    @'
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DocumentTier {
    Constitutional,
    Operational,
    Ephemeral,
}

#[derive(Serialize, Deserialize)]
struct SigMeta {
    signature: String,
    tier: DocumentTier,
    signed_at: chrono::DateTime<chrono::Utc>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: sign-docs <output-dir> <templates-dir>");
        eprintln!("  Generates keypair in output-dir and signs templates.");
        std::process::exit(1);
    }

    let output_dir = Path::new(&args[1]);
    let templates_dir = Path::new(&args[2]);

    // Generate keypair
    let signing_key = SigningKey::generate(&mut OsRng);
    let pubkey = signing_key.verifying_key();

    // Save keys
    fs::create_dir_all(output_dir).expect("create output dir");
    fs::write(output_dir.join("signing_key.bin"), signing_key.to_bytes()).expect("write signing key");
    fs::write(output_dir.join("pubkey.bin"), pubkey.to_bytes()).expect("write pubkey");

    println!("Generated keypair:");
    println!("  Private: {}/signing_key.bin (KEEP SECURE)", output_dir.display());
    println!("  Public:  {}/pubkey.bin", output_dir.display());

    // Sign each template
    let docs = ["soul.md", "ethics.md", "instincts.md"];
    for doc_name in docs {
        let doc_path = templates_dir.join(doc_name);
        if !doc_path.exists() {
            eprintln!("Warning: {} not found, skipping", doc_path.display());
            continue;
        }

        let content = fs::read_to_string(&doc_path).expect("read doc");
        
        // Create signable bytes (name + tier + content)
        let tier = DocumentTier::Constitutional;
        let signable = format!("{}|{:?}|{}", doc_name, tier, content);
        let signature = signing_key.sign(signable.as_bytes());

        let meta = SigMeta {
            signature: BASE64.encode(signature.to_bytes()),
            tier,
            signed_at: chrono::Utc::now(),
        };

        let sig_path = templates_dir.join(format!("{}.sig", doc_name));
        let json = serde_json::to_string_pretty(&meta).expect("serialize sig");
        fs::write(&sig_path, &json).expect("write sig");

        println!("Signed: {} -> {}", doc_name, sig_path.display());
    }

    println!("\nDone! Next steps:");
    println!("1. Store {}/signing_key.bin securely (NOT in repo)", output_dir.display());
    println!("2. Set external_pubkey_path in Abigail config to {}/pubkey.bin", output_dir.display());
    println!("3. Commit the .sig files in templates/");
}
'@ | Out-File -FilePath (Join-Path $toolDir "src" "main.rs") -Encoding utf8
}

# Build the signing tool
Write-Host "Building signing tool..."
Push-Location $toolDir
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to build signing tool"
    }
} finally {
    Pop-Location
}

# Run the signing tool
$templatesDir = Join-Path $PSScriptRoot ".." "templates"
$signTool = Join-Path $toolDir "target" "release" "sign-docs.exe"

Write-Host "Running signing tool..."
& $signTool $OutputDir $templatesDir

if ($LASTEXITCODE -ne 0) {
    throw "Signing tool failed"
}

Write-Host ""
Write-Host "SUCCESS! Keypair generated and templates signed."
Write-Host ""
Write-Host "IMPORTANT SECURITY NOTES:"
Write-Host "  1. The private key at $OutputDir\signing_key.bin must be kept secure"
Write-Host "  2. NEVER commit the private key to version control"
Write-Host "  3. Set OS permissions on $OutputDir to restrict access"
Write-Host "  4. The public key at $OutputDir\pubkey.bin should be read-only for Abigail"
Write-Host ""
Write-Host "To configure Abigail, set external_pubkey_path in config.json or example.env"
