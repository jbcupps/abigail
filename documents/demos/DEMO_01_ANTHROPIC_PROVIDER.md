# Demo 1: Chat with Claude in AO

**Cycle:** C1 — Anthropic Claude Provider
**Branch:** `claude/assess-openclaw-gaps-tNhHT`
**What's new:** AO can now use Anthropic Claude as its Ego (cloud) provider, not just OpenAI.

---

## Prerequisites
- An Anthropic API key (starts with `sk-ant-`)
- OR an OpenAI API key (starts with `sk-`)
- A local LLM running (Ollama or LM Studio) — optional but recommended for Id routing

## Setup Commands

```bash
# 1. Pull the latest code
git fetch origin
git checkout claude/assess-openclaw-gaps-tNhHT
git pull origin claude/assess-openclaw-gaps-tNhHT

# 2. Build and run
cargo tauri dev
```

## Test Steps

### Test 1: Store an Anthropic API Key
1. Open AO (it should launch after `cargo tauri dev`)
2. If birth is already complete, go to the chat interface
3. In the chat, type: **"My Anthropic API key is sk-ant-YOUR_KEY_HERE"**
4. AO should:
   - Recognize this as an API key
   - Validate it against the Anthropic API
   - Store it in the secure vault
   - Rebuild the router to use Claude as Ego
5. **Expected:** You see a confirmation like "Successfully validated and stored anthropic API key"

### Test 2: Verify Router Status
1. Open the browser dev console (F12 → Console)
2. Run: `await window.__TAURI__.invoke('get_router_status')`
3. **Expected:** You should see:
   ```json
   {
     "id_provider": "local_http" or "candle_stub",
     "id_url": "http://localhost:11434" or null,
     "ego_configured": true,
     "ego_provider": "anthropic",
     "routing_mode": "egoprimary"
   }
   ```
4. The key field is `"ego_provider": "anthropic"` — this confirms Claude is wired as the Ego.

### Test 3: Chat with Claude
1. Type a complex question: **"Explain the difference between Ed25519 and RSA signatures in 3 sentences."**
2. **Expected:** The response comes from Claude (you may notice the writing style differs from GPT)
3. Type a simple question: **"What is 2 + 2?"**
4. **Expected:** If a local LLM is running, this routes to Id (local). If not, falls back to Claude.

### Test 4: Verify Tool Calling Works
1. If you have a Tavily API key stored, type: **"Search the web for latest Rust news"**
2. **Expected:** Claude calls the web_search tool, gets results, and summarizes them
3. This confirms Anthropic's tool-use format mapping works correctly

## What Changed (Summary)

| File | Change |
|------|--------|
| `crates/ao-capabilities/src/cognitive/anthropic.rs` | **NEW** — Full Anthropic Messages API provider |
| `crates/ao-capabilities/src/cognitive/mod.rs` | Added `AnthropicProvider` export |
| `crates/ao-router/src/router.rs` | Ego now `Arc<dyn LlmProvider>`, new `with_provider()` constructor |
| `crates/ao-router/src/lib.rs` | Re-exports `EgoProvider` enum |
| `tauri-app/src/lib.rs` | Router rebuilds for Anthropic keys, startup auto-detects best provider |

## Known Limitations (Will Be Addressed in Later Cycles)
- No streaming yet (responses arrive all at once) — **Cycle 2**
- No UI dropdown to switch between OpenAI/Anthropic — uses whichever key is stored
- Model is hardcoded to `claude-sonnet-4-20250514` — model selection coming later
