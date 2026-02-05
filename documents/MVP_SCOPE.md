# MVP Scope

**MVP** = User clicks **Start** → AO runs **LLM heartbeat** → **signature verification** of constitutional docs → AO is informed they're OK → **user can talk to AO in the UI**.

## Startup order

1. **LLM heartbeat** — Verify local LLM is reachable (or use in-process stub if no URL configured).
2. **Signature verification** — Verify constitutional docs (soul.md, ethics.md, instincts.md) against external public key (or skip in dev mode if no pubkey configured).
3. **AO informed OK** — If checks pass, AO engages.
4. **Chat** — User can talk to AO via the local LLM.

## Out of scope for MVP

- **Email:** Not required. Birth flow skips email configuration.
- **Model download:** Not required. Uses external LLM server or in-process stub.
- **Skills scaffold:** Planned for **MVP+1**.

## External signing key

For production, the signing private key is created **out-of-band** (never stored in AO). AO only reads the public key from an external vault (file path or future KMS). In dev mode, signature verification is skipped.

See `documents/HOW_TO_RUN_LOCALLY.md` for the MVP run path and smoke test, and `documents/UAT_CHECKLIST.md` for manual UAT scenarios.
