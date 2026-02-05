# Security Notes

## Key Management

### External Signing Keypair (Ed25519)
- **Generated at first run** by the user's AO instance
- **Private key is shown ONCE** during initial setup - user MUST save it securely
- **Private key is NEVER stored** by AO - only the public key is retained
- **Public key location:** `{data_dir}/external_pubkey.bin` (auto-detected)
- **Purpose:** Signs constitutional documents (soul.md, ethics.md, instincts.md)

### Internal Keyring (Mentor Keypair)
- **Generated automatically** at first run
- **DPAPI-protected** on Windows (user scope), plaintext stub on other platforms (dev only)
- **Purpose:** Internal operations (signing memories, etc.)

## Storage Security

- **DPAPI:** Keyring and email passwords use Windows DPAPI when available
- **Non-Windows:** Plaintext stub with warning logged (for development only)
- **Keys file:** `{data_dir}/keys.bin` (DPAPI-encrypted)

## Secrets Handling

- **No secrets in repo:** API keys, passwords never committed
- **Environment:** Use `example.env` as template; `.env` is gitignored
- **Email passwords:** Encrypted via DPAPI before storage in config

## Constitutional Document Integrity

- **Signed at first run:** soul.md, ethics.md, instincts.md are signed when keypair is generated
- **Verified at every boot:** Signatures checked against the stored public key
- **Immutable:** AO refuses requests to modify constitutional docs
- **Recovery:** If user loses private key, they cannot re-sign after reinstall

## First Run Security Flow

1. User clicks "Start" in boot sequence
2. AO generates Ed25519 keypair
3. Constitutional documents are signed with the private key
4. **CRITICAL:** Private key is displayed with security warnings
5. User must acknowledge they've saved the key before proceeding
6. Private key is cleared from memory (never stored)
7. Only the public key remains for future verification

## Threat Model Summary

| Threat | Mitigation |
|--------|------------|
| Tampered constitutional docs | Signature verification at boot |
| Lost private key | Clear warnings during setup; user responsibility |
| Compromised private key | User can detect via failed verification |
| Man-in-the-middle on download | Installer signatures (future: code signing) |
| Local privilege escalation | DPAPI uses user scope, not machine scope |
