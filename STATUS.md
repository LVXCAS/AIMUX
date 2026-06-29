# AIMUX — Honest Status

AIMUX is a model-agnostic fork of OpenAI Codex (`codex-rs/`). The goal: the same
agent loop + ratatui TUI, runnable against Claude / Gemini / Mistral (and any
OpenAI-Chat-Completions-compatible endpoint) in addition to OpenAI, with an
optional router that auto-picks a provider per prompt and learns from overrides.

**Binary name:** `aimux` (see `codex-rs/cli/Cargo.toml` `[[bin]]`).
**Build:** `cd codex-rs && cargo build --bin aimux` — **green** as of this writing.
**Runs:** `./target/debug/aimux --version` and `--help` work; help text is rebranded "AIMUX".

> **Overall honesty note:** Everything below **COMPILES** and the binary **RUNS**.
> **No provider has been exercised with a real API key.** The Chat wire-format
> bridge (request build + SSE parse + tool-call reassembly) is implemented and
> type-checked end-to-end, but **no live request has been sent to Anthropic,
> Gemini, or Mistral**, and the **tool-calling round-trip is UNVERIFIED** against
> any real provider stream. Treat M2/M3 as "code-complete, runtime-unverified".

---

## Milestone status

| # | Milestone | Status | Notes |
|---|---|---|---|
| **M0** | Fork builds unchanged | ✅ COMPILES | Workspace builds; `aimux` bin runs. |
| **M1** | Chat wire format restored | ✅ COMPILES, ⚠️ UNVERIFIED live | `WireApi::Chat` re-introduced; request builder + SSE parser implemented. No live text round-trip exercised. |
| **M2** | Second provider proof-of-life | ⚠️ UNVERIFIED-pending-live-API-key | Anthropic/Gemini/Mistral are wired as built-in providers (`WireApi::Chat`). Never run against a real key, so a real streaming turn in the TUI is unproven. |
| **M3** | Tool-calling parity | ⚠️ UNVERIFIED (highest risk) | Tool-call reassembly is implemented (`codex-api/src/sse/chat.rs` accumulates `delta.tool_calls[]` → `ResponseItem::FunctionCall`; request tools via `create_tools_json_for_chat_api`). **Not validated against any provider's real stream.** This is the single biggest unknown — provider tool-call encoding quirks (Gemini/Mistral) could break the round-trip silently. |
| **M4** | Router | ✅ COMPILES, 🔒 GATED/NOT-LIVE-WIRED | `core/src/router/` ported from the TS prototype (complexity, task-type, route, learning store). Gated behind `router_enabled` config (default **false**). The pure decision fn `resolve_provider_override` exists and is unit-tested, but is **not yet called** at the live provider-resolution site — so even when enabled it does not currently change runtime routing (see `TODO(live-hook)` in `router/mod.rs`). |
| **M5** | Learning persistence | ✅ COMPILES, 🔒 NOT-LIVE-WIRED | `LearningStore` persists to `<home>/router_state.json`; `record_run` / `record_override` exist and are unit-tested. Not invoked from the live turn lifecycle yet (depends on M4 live-hook). No `/stats` view. |
| **M6** | Rebrand | ✅ COMPILES & VISIBLE | Bin `aimux`, help "AIMUX CLI", env/home rebrand touched in `cli/src/main.rs`, `utils/home-dir`, `login/auth`. Cosmetic; verified via `--help`. |
| **M7** | Native bridges (Anthropic Messages / Gemini generateContent) | ❌ NOT STARTED | Out of scope. Chat-compat path is the only route to non-OpenAI providers. |

### Legend
- ✅ **COMPILES** — type-checks and links into the green build.
- ⚠️ **UNVERIFIED-pending-live-API-key** — code path exists but has never executed against a real provider; correctness at runtime is unproven.
- 🔒 **GATED / NOT-LIVE-WIRED** — present and tested in isolation, but disabled by default and/or not yet called from the live execution path; no runtime behavior change.
- ❌ **NOT STARTED**.

---

## The three new providers + how to configure a key

All three are built-in (no `config.toml` `[model_providers]` block required) and
speak `WireApi::Chat` against the provider's OpenAI-compatible endpoint. Defined
in `codex-rs/model-provider-info/src/lib.rs`.

| Provider id | Base URL | Env key | Source |
|---|---|---|---|
| `anthropic` | `https://api.anthropic.com/v1` | `ANTHROPIC_API_KEY` | `create_chat_provider("Anthropic", …)` |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta/openai` | `GEMINI_API_KEY` | `create_chat_provider("Gemini", …)` |
| `mistral` | `https://api.mistral.ai/v1` | `MISTRAL_API_KEY` | `create_chat_provider("Mistral", …)` |

### Configuring a key (intended usage — UNVERIFIED end-to-end)
1. Export the matching env var, e.g. `export ANTHROPIC_API_KEY=sk-ant-...`.
2. Point the session at the provider via `~/.aimux/config.toml`:
   ```toml
   model_provider = "anthropic"
   model = "claude-..."   # a model the chat endpoint serves
   ```
   (or the equivalent CLI flags).
3. The default provider remains `openai` with base URL `https://api.openai.com/v1`
   — unchanged. Setting a non-OpenAI provider opts into the `WireApi::Chat` path.

> These steps are the **designed** flow. They have **not** been run against a
> live key, so failures (auth header shape, model-id mismatch, SSE quirks,
> tool-call encoding) may surface only when a real request is made.

### Optional: router (experimental, off by default)
```toml
router_enabled = true   # currently still a NO-OP at runtime; see M4 note above
```

---

## Build / run verification

```shell
cd codex-rs
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --bin aimux   # green
./target/debug/aimux --version                              # codex-cli 0.0.0
./target/debug/aimux --help                                 # "AIMUX CLI"
```

## What is explicitly NOT verified
- No live API call to **any** provider (OpenAI included on the Chat path).
- Tool-calling round-trip (apply_patch / shell / MCP) over `WireApi::Chat` — **unproven**.
- Streaming a full turn in the TUI from a non-OpenAI provider.
- Router actually changing the selected provider at runtime (live-hook not wired).
- Learning-store updates from real turn outcomes.
