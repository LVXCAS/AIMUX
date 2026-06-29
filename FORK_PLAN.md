# AIMUX — Codex Fork Plan

**Goal:** Fork OpenAI Codex (Rust, `codex-rs/`) into a *model-agnostic* coding agent — the
same agent loop + ratatui TUI, runnable on Claude / Gemini / Mistral / OpenAI, with a
router that auto-picks the provider per prompt and learns from manual overrides.

This plan is grounded in a full read of the real source (provider seam, core loop, auth,
TUI contract, config). Line/file references are to `codex-rs/` in this repo.

---

## 1. Verdict — much more tractable than "rewrite the model layer"

Four of the five things a model-agnostic fork needs **already exist** in Codex:

| Capability | State | Evidence |
|---|---|---|
| Provider abstraction | ✅ Exists, load-bearing | `ModelProviderInfo`, `ModelProvider` trait, `models-manager`; Amazon Bedrock is a working non-trivial example (`model-provider/src/amazon_bedrock/`) |
| Per-provider **API-key** auth | ✅ Exists via config | `ModelProviderInfo.env_key` → reads `ANTHROPIC_API_KEY` etc.; `AuthMode::ApiKey` already a codepath |
| TUI render path | ✅ Already provider-agnostic | `ServerNotification` enum + `chatwidget` delta handlers have **no** `gpt-`/OpenAI hardcoding in the render path |
| Router hook point | ✅ Clean seam | `build_models_manager()` (`core/src/thread_manager.rs:262`) and `Session::steer_input()` (`core/src/session/mod.rs:814`) |
| **Wire format** | ❌ **The one real gap** | `WireApi` has a **single** variant: `Responses`. `Chat` was removed. |

**The entire difficulty collapses to one problem: the wire format.**
Codex speaks *only* the OpenAI Responses API. It doesn't translate — it requires every
backend (even ollama) to natively serve `/v1/responses`. Claude, Gemini, and Mistral do
**not** serve the Responses API; they serve chat-completions / Messages / generateContent.
The `responses-api-proxy` crate is a pass-through, not a translator. So there is **no
config-only path** to a second provider.

## 2. The highest-leverage move: re-introduce `WireApi::Chat`

Don't start with a native Anthropic provider. Start by **resurrecting the Chat Completions
wire format** Codex deleted. Reason: chat-completions is the lingua franca.

- **Anthropic, Google Gemini, and Mistral all expose OpenAI-Chat-Completions-compatible endpoints.**
  - Mistral: `https://api.mistral.ai/v1` (native chat-completions)
  - Gemini: `https://generativelanguage.googleapis.com/v1beta/openai/`
  - Anthropic: OpenAI-compat chat endpoint on `api.anthropic.com`
- One `WireApi::Chat` bridge therefore unlocks **all three at once** (plus Groq, Together,
  OpenRouter, local OpenAI-compat servers) using only config + an API key.
- Reference code exists: Codex *had* a Chat implementation before the cutover. Recover it
  from git history / an older tag (`rust-v0.*` pre-removal) rather than writing from zero.

Native per-provider bridges (`WireApi::Anthropic` Messages API, `WireApi::Gemini`
generateContent) become **optional optimizations** later — for prompt caching, extended
thinking, and provider-specific features — not prerequisites.

### What the Chat bridge must do
The internal canonical type is `ResponseEvent` (`codex-api/src/common.rs:74`); everything
downstream (and the TUI) consumes it. The bridge implements, for `wire_api = "chat"`:

1. **Request build:** `ResponsesApiRequest` → chat-completions JSON
   (`messages[]` instead of `input[]`, `tools[]` as chat-style function schemas).
   Touch points: `core/src/client.rs` `build_responses_request()`,
   `tools/src/tool_spec.rs:80` `create_tools_json_for_responses_api()` (add a chat variant).
2. **SSE parse:** chat-completions stream (`choices[].delta.content`,
   `choices[].delta.tool_calls[]`, `finish_reason`) → `ResponseEvent` variants
   (`OutputTextDelta`, `ToolCallInputDelta`, `OutputItemDone(FunctionCall)`, `Completed`).
   New parser alongside `codex-api/src/sse/responses.rs`.
3. **Tool round-trip:** map chat `tool_calls` ↔ `ResponseItem::FunctionCall` /
   `FunctionCallOutput` (`protocol/src/models.rs:934`). **This is the crux** (see §6).
4. **Error remap:** implement `provider.map_api_error()` (`client.rs:2008`) for the new path.

## 3. Auth — API keys first, OAuth never (for v1)

Per-provider **API-key** auth already works through `ModelProviderInfo.env_key`. v1 needs
**nothing** in the OpenAI-OAuth machinery (`login/`, ChatGPT JWT claims, token refresh at
`auth.openai.com`) — all of that stays OpenAI-only and untouched.

- Claude / Gemini / Mistral → read `ANTHROPIC_API_KEY` / `GEMINI_API_KEY` / `MISTRAL_API_KEY`
  via `env_key`, or store in `auth.json` keyed per provider.
- Generalizing the OAuth flow to per-provider (Anthropic OAuth, Google OAuth) is **large**
  and explicitly **out of scope** until after the agent works on API keys. Don't touch it.

## 4. Router + learning (port from the TS prototype)

The TS prototype at `~/aimux-ts-prototype` has the logic to port (complexity estimation,
task-type detection, override-confidence). Reimplement as a small Rust module.

- **Hook site:** provider resolution at `core/src/config/mod.rs:3403` (default-provider
  selection) for session default, and `build_models_manager()`
  (`core/src/thread_manager.rs:262`) / `Session::steer_input()` for **per-turn** override.
- **Learning store:** usage/success/override counts persisted under the config dir
  (reuse the `~/.aimux` home; see §5). Port `OVERRIDE_CONFIDENCE_THRESHOLD` logic verbatim.

## 5. Rebrand surface (cosmetic — do it LAST)

Mechanical and pervasive; zero architectural risk. Enumerated:

- Binary name `codex` → `aimux`: `cli/src/main.rs:103` (`bin_name`), `cli/Cargo.toml` `[[bin]]`.
- Help text "Codex CLI": `cli/src/main.rs:91`.
- Home dir `~/.codex` → `~/.aimux`: `utils/home-dir/src/lib.rs:59`; env `CODEX_HOME`→`AIMUX_HOME` (`:14`).
- Env vars `CODEX_*` → `AIMUX_*`: `CODEX_API_KEY`, `CODEX_ACCESS_TOKEN`, etc. across `cli/src/main.rs`.
- Keyring service "Codex Auth" → "AIMUX Auth"; secret name `CODEX_AUTH` → `AIMUX_AUTH` (`login/src/auth/storage.rs`).
- Default provider stays `openai`; default base_url `https://api.openai.com/v1` unchanged.
- Leave `ReasoningEffort`/`openai_models.rs` named as-is initially (see §6 risk).

## 6. Build order (dependency-ordered) + highest risk

| # | Milestone | Definition of done | Effort |
|---|---|---|---|
| **M0** | Fork builds unchanged | `cargo build` green on the untouched clone; baseline binary runs | S* |
| **M1** | Chat wire format restored | `WireApi::Chat` variant + request build + SSE parse compiles; round-trips a **plain text** turn against an OpenAI-compat endpoint | M |
| **M2** | Second provider, proof-of-life | A **non-OpenAI** model (Mistral or Gemini via compat) renders a full streaming turn in the **unmodified TUI**, authed by API key in config | M |
| **M3** | Tool-calling parity | The agent successfully calls a tool (e.g. shell / `apply_patch`) through the Chat bridge and applies the result — the real bar for "it's an agent, not a chatbot" | **L (highest risk)** |
| **M4** | Router | TS complexity/task-type/override logic ported; auto-selects provider per turn | M |
| **M5** | Learning persistence | usage/success/overrides saved under `~/.aimux`, surfaced via a `/stats`-style view | S–M |
| **M6** | Rebrand | binary, home dir, env vars, strings (§5) | S (pervasive) |
| **M7 (opt.)** | Native bridges | `WireApi::Anthropic` (Messages + thinking + caching), `WireApi::Gemini` | L each |

\* M0 is "small" in changes but the build itself is a ~600k-LOC Rust workspace — expect a long
first compile and heavy dependency fetch.

**Single highest-risk unknown — M3 tool-calling fidelity.** Streaming *text* onto
`ResponseEvent` is straightforward. The danger is the **tool-call round-trip**: Codex's
agent loop is built around the Responses API's function-call item shape
(`ResponseItem::FunctionCall { name, arguments, call_id }` + `FunctionCallOutput`). Each
chat-completions provider encodes tool calls slightly differently (streamed
`delta.tool_calls` fragments, argument chunking, parallel-call IDs, and quirks — Gemini and
Mistral each deviate). If the bridge doesn't reassemble these into exactly what the agent
loop expects, tools (apply_patch, shell, MCP) silently break and the "agent" degrades to a
chat box. Prove M3 on one provider before scaling.

Secondary risk: `ReasoningEffort` (`protocol/src/openai_models.rs`) is threaded through
config/TUI and assumes an OpenAI concept. Non-reasoning providers must map it to `None`
cleanly; generalizing the enum is medium and can wait.

## 7. First concrete commit

1. Commit this `FORK_PLAN.md`.
2. **M0:** `cargo build` the untouched fork; capture the baseline. Fix any toolchain/version
   pins so it's green. Commit "chore: baseline build of codex fork".
3. **M1 spike:** recover the pre-removal Chat implementation from git history as the
   starting point for the `WireApi::Chat` bridge.
