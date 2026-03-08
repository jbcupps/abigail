//! Encrypted portable archive - export/restore memory data that survives
//! reinstalls. Uses hybrid encryption: AES-256-GCM for data, X25519 for
//! key wrapping (derived from the Ed25519 keypair generated at first run).

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng, Payload},
    Aes256Gcm, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use std::path::{Path, PathBuf};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519Public, StaticSecret};

use crate::store::MemoryStore;

const ARCHIVE_VERSION_V1: u32 = 1;
const ARCHIVE_VERSION_V2: u32 = 2;
const ARCHIVE_VERSION: u32 = ARCHIVE_VERSION_V2;
const NONCE_LEN: usize = 12;

/// Convert an Ed25519 public key (32 bytes) to an X25519 public key.
pub fn ed25519_pub_to_x25519(pub_bytes: &[u8; 32]) -> X25519Public {
    use ed25519_dalek::VerifyingKey;
    let vk = VerifyingKey::from_bytes(pub_bytes).expect("valid Ed25519 public key");
    let ep = vk.to_montgomery();
    X25519Public::from(ep.to_bytes())
}

/// Convert an Ed25519 private key to an X25519 static secret.
pub fn ed25519_priv_to_x25519(signing_key: &ed25519_dalek::SigningKey) -> StaticSecret {
    use sha2::{Digest, Sha512};
    let hash = Sha512::digest(signing_key.to_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&hash[..32]);
    key[0] &= 248;
    key[31] &= 127;
    key[31] |= 64;
    StaticSecret::from(key)
}

/// Serializable archive payload.
#[derive(serde::Serialize, serde::Deserialize)]
struct ArchivePayload {
    version: u32,
    exported_at: String,
    turns: Vec<crate::store::ConversationTurn>,
    memories: Vec<SerializableMemory>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SerializableMemory {
    id: String,
    content: String,
    weight: String,
    created_at: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ArchiveHeaderV2 {
    version: u32,
    key_exchange: String,
    key_derivation: String,
    cipher: String,
    ephemeral_public_key_base64: String,
    nonce_base64: String,
    exported_at: String,
}

/// Archive exporter/importer.
pub struct ArchiveExporter {
    pub_key_path: PathBuf,
    archive_dir: PathBuf,
}

impl ArchiveExporter {
    pub fn new(pub_key_path: PathBuf, archive_dir: PathBuf) -> Self {
        Self {
            pub_key_path,
            archive_dir,
        }
    }

    /// Build an exporter using the default Documents/Abigail/archives/ path.
    pub fn with_defaults(pub_key_path: PathBuf, agent_name: Option<&str>) -> Option<Self> {
        let docs = directories::UserDirs::new()?.document_dir()?.to_path_buf();
        let archive_dir = docs.join("Abigail").join("archives");
        let _ = std::fs::create_dir_all(&archive_dir);
        let _ = agent_name; // reserved for per-agent subdirs
        Some(Self {
            pub_key_path,
            archive_dir,
        })
    }

    /// Export all conversation turns + memories as an encrypted `.abigail` archive.
    pub fn export(&self, store: &MemoryStore) -> anyhow::Result<PathBuf> {
        let pub_bytes = std::fs::read(&self.pub_key_path)?;
        if pub_bytes.len() != 32 {
            anyhow::bail!("Invalid public key file (expected 32 bytes)");
        }
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&pub_bytes);
        let x_pub = ed25519_pub_to_x25519(&pk);

        let turns = store.all_turns()?;
        let memories = store
            .all_memories()?
            .into_iter()
            .map(|m| SerializableMemory {
                id: m.id,
                content: m.content,
                weight: m.weight.as_str().to_string(),
                created_at: m.created_at.to_rfc3339(),
            })
            .collect();

        let exported_at = chrono::Utc::now().to_rfc3339();
        let payload = ArchivePayload {
            version: ARCHIVE_VERSION,
            exported_at: exported_at.clone(),
            turns,
            memories,
        };

        let json = serde_json::to_vec(&payload)?;
        let eph_secret = EphemeralSecret::random_from_rng(OsRng);
        let eph_public = x25519_dalek::PublicKey::from(&eph_secret);
        let shared = eph_secret.diffie_hellman(&x_pub);
        let aead_key = derive_archive_key(shared.as_bytes())?;

        let cipher = Aes256Gcm::new_from_slice(&aead_key)?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let header = ArchiveHeaderV2 {
            version: ARCHIVE_VERSION_V2,
            key_exchange: "x25519".to_string(),
            key_derivation: "hkdf-sha256".to_string(),
            cipher: "aes-256-gcm".to_string(),
            ephemeral_public_key_base64: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                eph_public.as_bytes(),
            ),
            nonce_base64: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                nonce_bytes,
            ),
            exported_at,
        };
        let header_bytes = serde_json::to_vec(&header)?;
        let header_len = u32::try_from(header_bytes.len())
            .map_err(|_| anyhow::anyhow!("Archive header is too large"))?;

        let mut out = Vec::with_capacity(8 + header_bytes.len() + json.len() + 16);
        out.extend_from_slice(&ARCHIVE_VERSION_V2.to_le_bytes());
        out.extend_from_slice(&header_len.to_le_bytes());
        out.extend_from_slice(&header_bytes);

        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: json.as_ref(),
                    aad: &out,
                },
            )
            .map_err(|e| anyhow::anyhow!("AES-GCM encrypt failed: {}", e))?;
        out.extend_from_slice(&ciphertext);

        let _ = std::fs::create_dir_all(&self.archive_dir);
        let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("abigail_{}.abigail", ts);
        let path = self.archive_dir.join(&filename);

        abigail_core::secure_fs::write_bytes_atomic(&path, &out)?;
        tracing::info!("Archive exported to {}", path.display());
        Ok(path)
    }

    /// Restore from an encrypted archive using the Ed25519 recovery key.
    pub fn restore(
        archive_path: &Path,
        recovery_key_base64: &str,
        store: &MemoryStore,
    ) -> anyhow::Result<(usize, usize)> {
        let data = std::fs::read(archive_path)?;
        if data.len() < 4 {
            anyhow::bail!("Archive file too short");
        }

        let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

        let priv_bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            recovery_key_base64.trim(),
        )?;
        if priv_bytes.len() != 32 {
            anyhow::bail!("Recovery key must be 32 bytes (base64-encoded)");
        }
        let signing_key = ed25519_dalek::SigningKey::from_bytes(priv_bytes.as_slice().try_into()?);
        let x_secret = ed25519_priv_to_x25519(&signing_key);

        let plaintext = match version {
            ARCHIVE_VERSION_V1 => decrypt_archive_v1(&data, &x_secret)?,
            ARCHIVE_VERSION_V2 => decrypt_archive_v2(&data, &x_secret)?,
            other => anyhow::bail!("Unsupported archive version: {}", other),
        };

        let payload: ArchivePayload = serde_json::from_slice(&plaintext)?;

        let mut turns_imported = 0usize;
        for turn in &payload.turns {
            if store.insert_turn(turn).is_ok() {
                turns_imported += 1;
            }
        }

        let mut mems_imported = 0usize;
        for m in &payload.memories {
            let weight = match m.weight.as_str() {
                "distilled" => crate::store::MemoryWeight::Distilled,
                "crystallized" => crate::store::MemoryWeight::Crystallized,
                _ => crate::store::MemoryWeight::Ephemeral,
            };
            let memory = crate::store::Memory {
                id: m.id.clone(),
                content: m.content.clone(),
                weight,
                created_at: chrono::DateTime::parse_from_rfc3339(&m.created_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            };
            if store.insert_memory(&memory).is_ok() {
                mems_imported += 1;
            }
        }

        tracing::info!(
            "Archive restored: {} turns, {} memories",
            turns_imported,
            mems_imported
        );
        Ok((turns_imported, mems_imported))
    }
}

fn derive_archive_key(shared_secret: &[u8; 32]) -> anyhow::Result<[u8; 32]> {
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(Some(b"abigail-archive-v2"), shared_secret);
    let mut key = [0u8; 32];
    hk.expand(b"archive:aead-key", &mut key)
        .map_err(|e| anyhow::anyhow!("HKDF expand failed: {}", e))?;
    Ok(key)
}

fn decrypt_archive_v1(data: &[u8], x_secret: &StaticSecret) -> anyhow::Result<Vec<u8>> {
    if data.len() < 4 + 32 + NONCE_LEN {
        anyhow::bail!("Archive file too short");
    }

    let mut eph_pub_bytes = [0u8; 32];
    eph_pub_bytes.copy_from_slice(&data[4..36]);
    let eph_public = X25519Public::from(eph_pub_bytes);

    let mut nonce_bytes = [0u8; NONCE_LEN];
    nonce_bytes.copy_from_slice(&data[36..36 + NONCE_LEN]);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = &data[36 + NONCE_LEN..];

    let shared = x_secret.diffie_hellman(&eph_public);
    let cipher = Aes256Gcm::new_from_slice(shared.as_bytes())?;
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("Decryption failed - wrong recovery key?"))
}

fn decrypt_archive_v2(data: &[u8], x_secret: &StaticSecret) -> anyhow::Result<Vec<u8>> {
    if data.len() < 8 {
        anyhow::bail!("Archive v2 file too short");
    }
    let header_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let header_end = 8 + header_len;
    if data.len() <= header_end {
        anyhow::bail!("Archive v2 header truncated");
    }

    let header_bytes = &data[8..header_end];
    let header: ArchiveHeaderV2 = serde_json::from_slice(header_bytes)?;
    if header.version != ARCHIVE_VERSION_V2 {
        anyhow::bail!("Archive header version mismatch: {}", header.version);
    }
    if header.key_exchange != "x25519"
        || header.key_derivation != "hkdf-sha256"
        || header.cipher != "aes-256-gcm"
    {
        anyhow::bail!("Unsupported archive v2 algorithm suite");
    }

    let eph_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        header.ephemeral_public_key_base64.trim(),
    )?;
    let eph_array: [u8; 32] = eph_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Archive v2 ephemeral public key must be 32 bytes"))?;
    let eph_public = X25519Public::from(eph_array);

    let nonce_vec = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        header.nonce_base64.trim(),
    )?;
    let nonce_bytes: [u8; NONCE_LEN] = nonce_vec
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Archive v2 nonce must be {} bytes", NONCE_LEN))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let shared = x_secret.diffie_hellman(&eph_public);
    let aead_key = derive_archive_key(shared.as_bytes())?;
    let cipher = Aes256Gcm::new_from_slice(&aead_key)?;
    let ciphertext = &data[header_end..];

    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: &data[..header_end],
            },
        )
        .map_err(|_| anyhow::anyhow!("Decryption failed - wrong recovery key or tampered archive"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ConversationTurn, Memory, MemoryStore};

    #[test]
    fn test_ed25519_to_x25519_roundtrip() {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();

        let x_pub = ed25519_pub_to_x25519(&verifying.to_bytes());
        let x_priv = ed25519_priv_to_x25519(&signing);
        let x_pub_from_priv = x25519_dalek::PublicKey::from(&x_priv);

        assert_eq!(x_pub.as_bytes(), x_pub_from_priv.as_bytes());
    }

    #[test]
    fn test_export_and_restore() {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();

        let tmp = std::env::temp_dir().join("abigail_archive_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let pk_path = tmp.join("external_pubkey.bin");
        std::fs::write(&pk_path, verifying.to_bytes()).unwrap();

        let src = MemoryStore::open_in_memory().unwrap();
        let turn = ConversationTurn::new("sess1", "user", "hello world");
        src.insert_turn(&turn).unwrap();
        src.insert_memory(&Memory::distilled("important fact".into()))
            .unwrap();

        let archive_dir = tmp.join("archives");
        let exporter = ArchiveExporter::new(pk_path, archive_dir);
        let archive_path = exporter.export(&src).unwrap();

        let dst = MemoryStore::open_in_memory().unwrap();
        let recovery_key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            signing.to_bytes(),
        );
        let (turns, mems) = ArchiveExporter::restore(&archive_path, &recovery_key, &dst).unwrap();

        assert_eq!(turns, 1);
        assert_eq!(mems, 1);
        assert_eq!(dst.session_turn_count("sess1").unwrap(), 1);
        assert_eq!(dst.count_memories().unwrap(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_restore_legacy_v1_archive() {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();

        let tmp = std::env::temp_dir().join("abigail_archive_test_v1");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let src = MemoryStore::open_in_memory().unwrap();
        src.insert_turn(&ConversationTurn::new("sess1", "user", "legacy"))
            .unwrap();

        let payload = ArchivePayload {
            version: ARCHIVE_VERSION_V1,
            exported_at: chrono::Utc::now().to_rfc3339(),
            turns: src.all_turns().unwrap(),
            memories: Vec::new(),
        };
        let json = serde_json::to_vec(&payload).unwrap();

        let mut pk = [0u8; 32];
        pk.copy_from_slice(&verifying.to_bytes());
        let x_pub = ed25519_pub_to_x25519(&pk);
        let eph_secret = EphemeralSecret::random_from_rng(OsRng);
        let eph_public = x25519_dalek::PublicKey::from(&eph_secret);
        let shared = eph_secret.diffie_hellman(&x_pub);
        let cipher = Aes256Gcm::new_from_slice(shared.as_bytes()).unwrap();
        let nonce_bytes = [7u8; NONCE_LEN];
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), json.as_ref())
            .unwrap();

        let archive_path = tmp.join("legacy.abigail");
        let mut out = Vec::new();
        out.extend_from_slice(&ARCHIVE_VERSION_V1.to_le_bytes());
        out.extend_from_slice(eph_public.as_bytes());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        std::fs::write(&archive_path, out).unwrap();

        let dst = MemoryStore::open_in_memory().unwrap();
        let recovery_key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            signing.to_bytes(),
        );
        let (turns, mems) = ArchiveExporter::restore(&archive_path, &recovery_key, &dst).unwrap();

        assert_eq!(turns, 1);
        assert_eq!(mems, 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_archive_v2_tampered_header_fails() {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();

        let tmp = std::env::temp_dir().join("abigail_archive_test_v2_tamper");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let pk_path = tmp.join("external_pubkey.bin");
        std::fs::write(&pk_path, verifying.to_bytes()).unwrap();

        let src = MemoryStore::open_in_memory().unwrap();
        src.insert_turn(&ConversationTurn::new("sess1", "user", "hello"))
            .unwrap();

        let exporter = ArchiveExporter::new(pk_path, tmp.join("archives"));
        let archive_path = exporter.export(&src).unwrap();
        let mut bytes = std::fs::read(&archive_path).unwrap();
        bytes[12] ^= 0x01;
        std::fs::write(&archive_path, &bytes).unwrap();

        let dst = MemoryStore::open_in_memory().unwrap();
        let recovery_key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            signing.to_bytes(),
        );
        assert!(ArchiveExporter::restore(&archive_path, &recovery_key, &dst).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
