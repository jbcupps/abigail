# Security Notes

- **DPAPI:** Keyring and email password use Windows DPAPI (user scope) when available. Non-Windows: plaintext stub with a warning (dev only).
- **Keys:** Install signing key is used once to sign constitutional docs, then discarded. Only the install public key and mentor keypair are stored (DPAPI-protected).
- **Secrets:** No API keys or passwords in repo. Use `example.env` and gitignore `.env`; store real values in env or secure storage.
- **Refusal:** Abby refuses requests to modify soul.md/ethics.md citing signature constraint (constitutional docs are signed at install).
