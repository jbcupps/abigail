use crate::document::{CoreDocument, DocumentTier};
use crate::error::{CoreError, Result};
use crate::keyring::Keyring;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::Signature;
use std::collections::HashMap;
use std::path::Path;

/// Metadata stored in .sig files (signature + tier + signed_at; content lives in .md).
#[derive(serde::Serialize, serde::Deserialize)]
struct SigMeta {
    pub signature: String,
    pub tier: DocumentTier,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

pub struct Verifier {
    keyring: Keyring,
    documents: HashMap<String, CoreDocument>,
}

impl Verifier {
    pub fn new(keyring: Keyring) -> Self {
        Self {
            keyring,
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
        let signature_bytes = BASE64.decode(&doc.signature).map_err(|e| {
            CoreError::Crypto(format!("Invalid signature encoding: {}", e))
        })?;

        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|e| CoreError::Crypto(format!("Invalid signature: {}", e)))?;

        let signable = doc.signable_bytes();
        let valid = self.keyring.verify_install_signature(&signable, &signature);

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
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use std::fs;

    #[test]
    fn test_tamper_detection() {
        let temp = std::env::temp_dir().join("abby_core_tamper_test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let (keyring, install_signing) = Keyring::generate(temp.clone()).unwrap();
        keyring.save().unwrap();

        let content = "I am Abby. This is my soul.";
        let mut doc = CoreDocument::new(
            "soul.md".into(),
            DocumentTier::Constitutional,
            content.into(),
        );
        let sig = install_signing.sign(&doc.signable_bytes());
        doc.signature = BASE64.encode(sig.to_bytes());

        let verifier = Verifier::new(Keyring::load(temp.clone()).unwrap());
        verifier.verify_document(&doc).unwrap();

        // Tamper: modify content; signature no longer matches
        doc.content = "I am Abby. This is TAMPERED.".into();

        let verifier2 = Verifier::new(Keyring::load(temp).unwrap());
        let result = verifier2.verify_document(&doc);
        assert!(result.is_err(), "Tampered document should fail verification");

        let _ = fs::remove_dir_all(std::env::temp_dir().join("abby_core_tamper_test"));
    }
}
