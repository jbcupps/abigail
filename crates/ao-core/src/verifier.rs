use crate::document::{CoreDocument, DocumentTier};
use crate::error::{CoreError, Result};
use crate::keyring::Keyring;
use crate::vault::ExternalVault;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use std::collections::HashMap;
use std::path::Path;

/// Metadata stored in .sig files (signature + tier + signed_at; content lives in .md).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SigMeta {
    pub signature: String,
    pub tier: DocumentTier,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

/// Verifier for constitutional documents.
///
/// Uses an **external public key** (from vault) as the trust root for signature verification.
/// The signing private key is never stored in AO.
pub struct Verifier {
    pubkey: VerifyingKey,
    documents: HashMap<String, CoreDocument>,
}

impl Verifier {
    /// Create a new verifier with a public key from an external vault.
    pub fn from_vault<V: ExternalVault>(vault: &V) -> Result<Self> {
        let pubkey = vault.read_public_key()?;
        Ok(Self {
            pubkey,
            documents: HashMap::new(),
        })
    }

    /// Create a new verifier with a directly provided public key.
    /// Useful for testing or when the key is already loaded.
    pub fn with_pubkey(pubkey: VerifyingKey) -> Self {
        Self {
            pubkey,
            documents: HashMap::new(),
        }
    }

    /// Load and verify all constitutional documents (soul.md, ethics.md, instincts.md).
    pub fn verify_soul(&mut self, docs_path: &Path) -> Result<()> {
        let required_docs = ["soul.md", "ethics.md", "instincts.md"];

        for doc_name in required_docs {
            let doc_path = docs_path.join(doc_name);
            let sig_path = docs_path.join(format!("{}.sig", doc_name));

            if !doc_path.exists() {
                return Err(CoreError::DocumentNotFound(doc_name.to_string()));
            }
            if !sig_path.exists() {
                return Err(CoreError::DocumentNotFound(format!("{}.sig", doc_name)));
            }

            let content = std::fs::read_to_string(&doc_path)?;
            let sig_json = std::fs::read_to_string(&sig_path)?;
            let meta: SigMeta = serde_json::from_str(&sig_json)
                .map_err(|e| CoreError::Config(format!("Invalid .sig JSON: {}", e)))?;

            let doc = CoreDocument {
                name: doc_name.to_string(),
                tier: meta.tier,
                content: content.clone(),
                signature: meta.signature,
                signed_at: meta.signed_at,
            };

            self.verify_document(&doc)?;
            self.documents.insert(doc_name.to_string(), doc);
        }

        tracing::info!("Soul verification complete. All documents authentic.");
        Ok(())
    }

    pub fn verify_document(&self, doc: &CoreDocument) -> Result<()> {
        let signature_bytes = BASE64
            .decode(&doc.signature)
            .map_err(|e| CoreError::Crypto(format!("Invalid signature encoding: {}", e)))?;

        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|e| CoreError::Crypto(format!("Invalid signature: {}", e)))?;

        let signable = doc.signable_bytes();
        let valid = Keyring::verify_signature(&self.pubkey, &signable, &signature);

        if !valid {
            return Err(CoreError::SignatureInvalid {
                document: doc.name.clone(),
            });
        }

        Ok(())
    }

    pub fn get_document(&self, name: &str) -> Option<&CoreDocument> {
        self.documents.get(name)
    }

    pub fn soul_content(&self) -> Option<&str> {
        self.documents.get("soul.md").map(|d| d.content.as_str())
    }

    pub fn ethics_content(&self) -> Option<&str> {
        self.documents.get("ethics.md").map(|d| d.content.as_str())
    }
}

/// Write a .sig file for a document (used at install/first-run after signing).
pub fn write_sig_file(docs_path: &Path, doc_name: &str, doc: &CoreDocument) -> Result<()> {
    let meta = SigMeta {
        signature: doc.signature.clone(),
        tier: doc.tier,
        signed_at: doc.signed_at,
    };
    let path = docs_path.join(format!("{}.sig", doc_name));
    let json = serde_json::to_string_pretty(&meta)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentTier;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use std::fs;

    #[test]
    fn test_tamper_detection() {
        let temp = std::env::temp_dir().join("ao_core_tamper_test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        // Generate an out-of-band signing key (simulating external key creation)
        let signing_key = SigningKey::generate(&mut OsRng);
        let pubkey = signing_key.verifying_key();

        let content = "I am AO. This is my soul.";
        let mut doc = CoreDocument::new(
            "soul.md".into(),
            DocumentTier::Constitutional,
            content.into(),
        );
        let sig = signing_key.sign(&doc.signable_bytes());
        doc.signature = BASE64.encode(sig.to_bytes());

        // Verify using the public key directly (simulating external vault)
        let verifier = Verifier::with_pubkey(pubkey);
        verifier.verify_document(&doc).unwrap();

        // Tamper: modify content; signature no longer matches
        doc.content = "I am AO. This is TAMPERED.".into();

        let verifier2 = Verifier::with_pubkey(pubkey);
        let result = verifier2.verify_document(&doc);
        assert!(
            result.is_err(),
            "Tampered document should fail verification"
        );

        let _ = fs::remove_dir_all(std::env::temp_dir().join("ao_core_tamper_test"));
    }

    #[test]
    fn test_repair_cycle() {
        use crate::keyring::{
            generate_external_keypair, parse_private_key, sign_constitutional_documents,
        };

        let temp = std::env::temp_dir().join("ao_core_repair_test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let docs_dir = temp.join("docs");
        let data_dir = temp.join("data");
        fs::create_dir_all(&docs_dir).unwrap();
        fs::create_dir_all(&data_dir).unwrap();

        // 1. Create dummy docs
        for doc in ["soul.md", "ethics.md", "instincts.md"] {
            fs::write(docs_dir.join(doc), "content").unwrap();
        }

        // 2. Generate keys (First Run)
        let key_result = generate_external_keypair(&data_dir).unwrap();
        let signing_key = parse_private_key(&key_result.private_key_base64).unwrap();

        // 3. Sign docs
        sign_constitutional_documents(&signing_key, &docs_dir).unwrap();

        // 4. Verify Success
        let vault = crate::vault::ReadOnlyFileVault::new(&key_result.public_key_path);
        let mut verifier = Verifier::from_vault(&vault).unwrap();
        verifier
            .verify_soul(&docs_dir)
            .expect("Verification should pass");

        // 5. Delete a sig (Corruption)
        fs::remove_file(docs_dir.join("soul.md.sig")).unwrap();

        // 6. Verify Failure
        let mut verifier2 = Verifier::from_vault(&vault).unwrap();
        assert!(
            verifier2.verify_soul(&docs_dir).is_err(),
            "Verification should fail"
        );

        // 7. Repair (Re-sign)
        sign_constitutional_documents(&signing_key, &docs_dir).unwrap();

        // 8. Verify Success again
        let mut verifier3 = Verifier::from_vault(&vault).unwrap();
        verifier3
            .verify_soul(&docs_dir)
            .expect("Verification should pass after repair");

        let _ = fs::remove_dir_all(&temp);
    }

    /// End-to-end identity lifecycle: keygen → sign → verify → tamper → detect → repair → verify.
    /// This simulates the full boot-to-verified path without GUI.
    #[test]
    fn test_full_identity_lifecycle() {
        use crate::keyring::{
            generate_external_keypair, parse_private_key, sign_constitutional_documents,
        };

        let temp = std::env::temp_dir().join("ao_core_lifecycle_test");
        let _ = fs::remove_dir_all(&temp);

        let data_dir = temp.join("data");
        let docs_dir = temp.join("docs");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&docs_dir).unwrap();

        // 1. Create realistic constitutional docs
        fs::write(docs_dir.join("soul.md"), "I am AO. My designation is AO.").unwrap();
        fs::write(
            docs_dir.join("ethics.md"),
            "Triangle Ethic: Deontological, Areteological, Teleological.",
        )
        .unwrap();
        fs::write(
            docs_dir.join("instincts.md"),
            "Privacy Prime: sanitize PII before cloud.",
        )
        .unwrap();

        // 2. Generate keypair (simulates first-run key generation)
        let key_result = generate_external_keypair(&data_dir).unwrap();
        assert!(data_dir.join("external_pubkey.bin").exists());
        assert!(!key_result.private_key_base64.is_empty());

        // 3. Parse private key back (simulates user providing saved key)
        let signing_key = parse_private_key(&key_result.private_key_base64).unwrap();

        // 4. Sign all constitutional documents
        sign_constitutional_documents(&signing_key, &docs_dir).unwrap();
        assert!(docs_dir.join("soul.md.sig").exists());
        assert!(docs_dir.join("ethics.md.sig").exists());
        assert!(docs_dir.join("instincts.md.sig").exists());

        // 5. Verify all signatures pass
        let vault = crate::vault::ReadOnlyFileVault::new(&key_result.public_key_path);
        let mut verifier = Verifier::from_vault(&vault).unwrap();
        verifier
            .verify_soul(&docs_dir)
            .expect("Initial verification should pass");

        // 6. Verify document content is accessible
        assert!(verifier.soul_content().unwrap().contains("AO"));
        assert!(verifier
            .ethics_content()
            .unwrap()
            .contains("Triangle Ethic"));

        // 7. Tamper with a document
        fs::write(docs_dir.join("ethics.md"), "TAMPERED: No ethics apply.").unwrap();
        let mut verifier2 = Verifier::from_vault(&vault).unwrap();
        let tamper_result = verifier2.verify_soul(&docs_dir);
        assert!(
            tamper_result.is_err(),
            "Tampered document should fail verification"
        );

        // 8. Restore original content
        fs::write(
            docs_dir.join("ethics.md"),
            "Triangle Ethic: Deontological, Areteological, Teleological.",
        )
        .unwrap();

        // 9. Re-sign (repair) all documents
        sign_constitutional_documents(&signing_key, &docs_dir).unwrap();

        // 10. Verify passes again after repair
        let mut verifier3 = Verifier::from_vault(&vault).unwrap();
        verifier3
            .verify_soul(&docs_dir)
            .expect("Verification should pass after repair");

        // 11. Test wrong private key is rejected
        let wrong_key = SigningKey::generate(&mut OsRng);
        sign_constitutional_documents(&wrong_key, &docs_dir).unwrap();
        let mut verifier4 = Verifier::from_vault(&vault).unwrap();
        let wrong_key_result = verifier4.verify_soul(&docs_dir);
        assert!(
            wrong_key_result.is_err(),
            "Wrong key signatures should fail verification"
        );

        // 12. Test missing document detection
        fs::remove_file(docs_dir.join("instincts.md")).unwrap();
        let mut verifier5 = Verifier::from_vault(&vault).unwrap();
        let missing_result = verifier5.verify_soul(&docs_dir);
        assert!(
            missing_result.is_err(),
            "Missing document should fail verification"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    /// Test cross-document tampering: tamper one doc while others remain valid.
    #[test]
    fn test_partial_tampering_detected() {
        use crate::keyring::{
            generate_external_keypair, parse_private_key, sign_constitutional_documents,
        };

        let temp = std::env::temp_dir().join("ao_core_partial_tamper");
        let _ = fs::remove_dir_all(&temp);

        let data_dir = temp.join("data");
        let docs_dir = temp.join("docs");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&docs_dir).unwrap();

        for doc in ["soul.md", "ethics.md", "instincts.md"] {
            fs::write(docs_dir.join(doc), format!("{} content", doc)).unwrap();
        }

        let key_result = generate_external_keypair(&data_dir).unwrap();
        let signing_key = parse_private_key(&key_result.private_key_base64).unwrap();
        sign_constitutional_documents(&signing_key, &docs_dir).unwrap();

        // Tamper with only instincts.md — verification should still fail
        fs::write(docs_dir.join("instincts.md"), "TAMPERED instincts").unwrap();

        let vault = crate::vault::ReadOnlyFileVault::new(&key_result.public_key_path);
        let mut verifier = Verifier::from_vault(&vault).unwrap();
        let result = verifier.verify_soul(&docs_dir);
        assert!(result.is_err(), "Partial tampering should be detected");

        let _ = fs::remove_dir_all(&temp);
    }
}
