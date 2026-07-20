# Ghost Prebuild — Multi-Provider Development Roadmap

**Branch**: `feat/povider-list-commands` (current: PR #5)  
**Objective**: Replicate Pi agent's provider/auth architecture — dual auth modes (OAuth subscription + API key), multiple models per provider, runtime provider switching, credential store with refresh.

**Reference implementation**: [earendil-works/pi](https://github.com/earendil-works/pi) — `packages/ai/src/`

**Deep research**: [DEEP_RESEARCH_PI.md](./DEEP_RESEARCH_PI.md) — Pi's 36-provider architecture, dynamic/static model system, credential store, OAuth flows

**Phases 2-5 plan**: [PLAN_PHASES_2_5.md](./PLAN_PHASES_2_5.md) — Combined execution plan (~15 PRs, ~4 weeks)

---

## Architecture Target (from Pi's Design)

### Auth Model

Pi has a clean two-arm auth design per provider:

| Arm | Use case | Interface |
|-----|----------|-----------|
| `ApiKeyAuth` | API keys (env vars, stored key, ambient files) | `name`, `login?()`, `check?()`, `resolve()` |
| `OAuthAuth` | Subscription/OAuth (ChatGPT Plus, Claude Pro, GitHub Copilot) | `name`, `loginLabel?`, `login()`, `refresh()`, `toAuth()` |

Both arms are combined into `ProviderAuth { apiKey?, oauth? }` — a provider can support one or both modes. Anthropic (Claude) is the canonical dual-auth provider: `ANTHROPIC_API_KEY` OR `ANTHROPIC_OAUTH_TOKEN` from env, plus OAuth for Claude Pro/Max.

**Auth resolution precedence** (from `resolveProviderAuth`):
1. Override API key (CLI flag `--api-key`) → wins if present
2. Stored credential (OAuth or API key) → from `CredentialStore`
3. Ambient env vars → `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.

**Credential Store** (`CredentialStore` interface):
- `read(providerId)` — retrieve stored credential
- `list()` — enumerate stored providers
- `modify(providerId, fn)` — serialized read-modify-write (the only write path)
- `delete(providerId)` — logout
- OAuth refresh runs inside `modify()` with double-checked locking — concurrent requests can't double-refresh

### Provider Model

```rust
// Target Rust equivalent (simplified)
pub struct Provider {
    pub id: String,          // "openai", "anthropic", "openai-codex"
    pub name: String,        // "OpenAI", "Anthropic"
    pub base_url: Option<String>,
    pub auth: ProviderAuth,
    pub models: Vec<Model>,  // static baseline + dynamic overlay
}

pub struct ProviderAuth {
    pub api_key: Option<ApiKeyAuth>,
    pub oauth: Option<OAuthAuth>,
}

pub trait CredentialStore {
    fn read(&self, provider_id: &str) -> Option<Credential>;
    fn list(&self) -> Vec<CredentialInfo>;
    fn modify(&self, provider_id: &str, fn: impl FnOnce(Option<Credential>) -> Option<Credential>) -> Option<Credential>;
    fn delete(&self, provider_id: &str);
}
```

### Model Catalog

Each provider has typed models with known APIs:

```rust
pub struct Model<TApi> {
    pub id: String,           // "gpt-5.6-sol", "claude-opus-4-7"
    pub provider: String,     // which provider owns this model
    pub api: TApi,            // typed API variant: openai-responses, anthropic-messages, etc.
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub cost: ModelCostRates,
    pub reasoning: Option<ReasoningConfig>,
    // ...
}
```

Providers can have:
- **Static models** — baked into the catalog (like our `default_models.json`)
- **Dynamic models** — fetched from the provider's API at startup (`refreshModels()`)

---

## Development Phases

### Phase 0: Foundation Audit (Do First)

**Goal**: Understand what already exists and what needs changing before writing code.

| ID | Task | Detail |
|----|------|--------|
| P0.1 | Map the sampler → HTTP client path | Trace from `xai-grok-sampler` through `xai-grok-shell` to understand where API key, base URL, and model ID meet the HTTP request. Document the data flow. |
| P0.2 | Map config loading | Trace from `xai-grok-config` → `config.toml` → CLI flags to understand where `ProviderRegistry` should be injected. |
| P0.3 | Audit existing auth credentials | `GrokAuthCredentials::with_api_key()` already exists — trace where it's called and how credentials flow into the sampler. |
| P0.4 | Identify all `GROK_*` env var readers | `grep -r "GROK_" crates/` to catalog all env var references for Phase 5 migration. |
| P0.5 | Define the `config.toml` schema | Design the TOML format for provider configuration, mirroring pi's provider lists + credential separation. |

### Phase 1: Wire Provider Switching (Core Integration)

**⚠️ Migration note (before Phase 2):** PR #1 shipped `ProviderAuthMode` as a mutually-exclusive enum (`ApiKey | Codex`). To support dual-auth providers (Anthropic with both API key + OAuth), the config schema must migrate from `auth_mode: Option<ProviderAuthMode>` to a dual-arm `ProviderAuth { api_key: Option<...>, oauth: Option<...> }` **before** Phase 2 OAuth lands. Without this migration, Anthropic dual-auth is unreachable. A `[providers.anthropic]` that wants both modes requires two config arms, not a single enum variant.

**Goal**: ✅ DONE (PR #3 + PR #4). `--provider` / `--api-key` CLI flags wired through full credential resolution.

| ID | Task | Priority | Detail | Status |
|----|------|----------|--------|--------|
| P1.1 | Add `--provider` CLI flag to `PagerArgs` | 🔴 Critical | In `xai-grok-pager/src/app/cli.rs`. Also `--api-key` flag. Both in `AgentArgs` + `HeadlessOptions`. | ✅ PR #3 |
| P1.2 | Load `ProviderRegistry` from `config.toml` | 🔴 Critical | `Config.providers: ProviderRegistry` deserialized from `[[providers]]` TOML. | ✅ PR #4 |
| P1.3 | Wire provider into credential resolution | 🔴 Critical | `resolve_credentials_with_override()` selects api_base URL + resolves api_key from provider. All 7 call sites updated. | ✅ PR #4 |
| P1.4 | Route API key into auth credentials | 🔴 Critical | Provider `resolve_api_key()` inserted in priority chain: `api_key_override > provider key > model key > session > XAI_API_KEY`. `GrokAuthCredentials::with_api_key()`. | ✅ PR #1 |
| P1.5 | Handle `api_base` URL selection | 🔴 Critical | Provider's `api_base` overrides model's base. Fallback to model.info.base_url. | ✅ PR #4 |
| P1.6 | `ghost provider list` command | 🟡 High | List all configured providers, their auth mode (API key / OAuth / Codex), model count, status. | ✅ PR #5 |
| P1.7 | `ghost models list [--provider]` command | 🟡 High | List models. Filter by provider. Show context windows, reasoning support, cost tiers. | ✅ PR #5 |
| P1.8 | Runtime `/provider` slash command | 🟢 Medium | In TUI: switch provider mid-session. Lists configured providers, shows current. | ⏳ |

### Phase 2: OAuth / Subscription Auth Flows

**Goal**: Support `ghost login <provider> --oauth` for subscription-based providers (OpenAI ChatGPT Plus/Pro, Anthropic Claude Pro/Max, GitHub Copilot).

**Status**: 🚧 In progress (PR #6 — P2.1-P2.6 delivered, P2.4 device-code + P2.7-P2.8 pending)

| ID | Task | Priority | Detail | Status |
|----|------|----------|--------|--------|
| P2.1 | `CredentialStore` trait + `FileCredentialStore` | 🔴 Critical | Trait in `xai-grok-auth/src/credential_store.rs`. JSON-backed at `~/.ghost/credentials.json`. `read`/`write`/`modify`/`delete`. | ✅ PR #6 |
| P2.2 | `Credential` enum (ApiKey + OAuth) | 🔴 Critical | `Credential::ApiKey { key }` + `Credential::OAuth { access_token, refresh_token, expires_at }`. Expiry checks. | ✅ PR #6 |
| P2.3 | Browser-based OAuth PKCE flow | 🔴 Critical | `oauth/pkce.rs` (code challenge) + `oauth/flow.rs` (browser open, local callback server, token exchange). | ✅ PR #6 |
| P2.4 | Device-code OAuth flow (headless) | 🟡 High | For headless/server environments. Display user code, poll for completion. | ⏳ |
| P2.5 | Token resolution from credential store | 🔴 Critical | `resolve_oauth_credential()` reads `credentials.json`, checks expiry, returns access token. | ✅ PR #6 |
| P2.6 | `ghost login <provider>` command | 🔴 Critical | `login_cmd.rs` — supports `--api-key` (direct key), `--oauth` (PKCE flow), codex auth. | ✅ PR #6 |
| P2.7 | `ghost logout <provider>` command | 🟡 High | Remove stored credential from store. | ⏳ |
| P2.8 | `ghost auth status` command | 🟢 Medium | Show which providers are authenticated, auth mode, account ID, token expiry. | ⏳ |

### Phase 3: Pi-Compatible Model Catalog

**Goal**: Mirror pi's `models.generated.ts` + `default_models.json` architecture with typed APIs and dynamic refresh.

| ID | Task | Priority | Detail |
|----|------|----------|--------|
| P3.1 | Typed API enum | 🟡 High | `enum ModelApi { OpenaiResponses, AnthropicMessages, OpenaiCompletions, ... }`. Each model declares its API. |
| P3.2 | Model struct with reasoning/cost | 🟡 High | `Model { id, provider, api, context_window, max_output_tokens, cost: CostRates, reasoning: Option<ReasoningConfig>, ... }` |
| P3.3 | Dynamic model refresh from provider APIs | 🟡 High | `refreshModels()`: restore from store, fetch from API (e.g. OpenAI `/models`), persist. Cache with TTL. |
| P3.4 | `filterModels()` by credential type | 🟢 Medium | Some models only available with OAuth (subscription-tier models). Filter based on credential type. |
| P3.5 | Model store persistence | 🟢 Medium | `~/.ghost/models_cache.json` — analogous to Codex's `models_cache.json`. |
| P3.6 | Embedded → ProviderConfig bridge | 🟡 High | Parse `EmbeddedProvider` fields into `ProviderConfig`. Convert `auth_mode` string → `ProviderAuthMode` enum with `warn-on-unknown`. |

### Phase 4: Full Crate Rename

**Goal**: `xai-grok-*` → `ghost-*`, `xai-*` → `ghost-*`.

| ID | Task | Priority | Detail |
|----|------|----------|--------|
| P4.1 | Rename 75 crate directories | 🔴 Critical | `xai-grok-*` → `ghost-*`, `xai-*` → keep shared. Scripted bulk rename with validation. |
| P4.2 | Update per-crate Cargo.toml files | 🔴 Critical | Package names, dependency references to sibling crates. |
| P4.3 | Update root Cargo.toml | 🔴 Critical | Workspace members, dependency paths, patch sections. |
| P4.4 | Global import rename | 🔴 Critical | `use xai_grok_*` → `use ghost_*`, `xai_grok::` → `ghost::`. |
| P4.5 | Binary artifact rename | 🟡 High | `xai-grok-pager` → `ghost`. Update CI, packaging, install scripts. |
| P4.6 | Internal string references | 🟡 High | "grok" → "ghost" in error messages, CLI help text, config keys. |

### Phase 5: Full Env Var Migration

**Goal**: All `GROK_*` env vars have `GHOST_*` alternatives with graceful fallback.

| ID | Task | Priority | Detail |
|----|------|----------|--------|
| P5.1 | Catalog all env var readers | 🟡 High | From P0.4 audit. Every `std::env::var("GROK_*")` site. |
| P5.2 | Add `GHOST_*` with fallback | 🟡 High | Replace each `std::env::var("GROK_FOO")` with `resolve_ghost_env("GHOST_FOO", "GROK_FOO", default)`. |
| P5.3 | Update docs and help text | 🟢 Medium | All references from GROK to GHOST. |

### Phase 6: Advanced Provider Features

**Goal**: Production-quality provider management.

| ID | Task | Priority | Detail |
|----|------|----------|--------|
| P6.1 | Provider health checks | 🟢 Medium | Ping each provider's API on startup. Mark unhealthy providers. Timeouts. |
| P6.2 | Per-provider context window configuration | 🟢 Medium | Allow overriding model context windows in config. Useful for proxies and rate-limited tiers. |
| P6.3 | Provider failover | 🟢 Medium | If primary provider fails, auto-fallback to next configured provider with the same model. |
| P6.4 | Usage/cost tracking per provider | 🟢 Medium | Track token usage and cost per provider. `ghost usage` command. |
| P6.5 | Config profiles | 🟢 Medium | Named config profiles (`ghost --profile work`). Different provider/model/defaults per profile. |
| P6.6 | User-provider config (custom) | 🟢 Medium | Users can add custom OpenAI-compatible providers in `~/.ghost/config.toml` without modifying the catalog. |

### Phase 7: Product Rebranding — `ghost-code`

**Goal**: Ship as `ghost-code` — a distinct product identity that separates the
OpenAI-compatible provider CLI from its xAI-grok fork origins.

| ID | Task | Priority | Detail |
|----|------|----------|--------|
| P7.1 | Binary rename | 🔴 Critical | `xai-grok-pager` → `ghost-code`. Update install scripts, CI artifacts, Homebrew/apt. |
| P7.2 | Branding sweep | 🔴 Critical | All user-facing strings: CLI help, error messages, README, docs, `ghost providers` output. "ghost-prebuild" → "ghost-code". |
| P7.3 | Config home rename | 🟡 High | `~/.ghost` → `~/.ghost-code` (with symlink fallback for migration). `GHOST_HOME` env var still works. |
| P7.4 | Git remote + release assets | 🟡 High | GitHub repo, release tags, binary artifacts — all reflect `ghost-code`. |
| P7.5 | Logo + visual identity | 🟢 Medium | Distinct terminal-friendly logo. Differentiate from Grok/xAI branding. |
| P7.6 | Package registry entries | 🟢 Medium | Crates.io (`ghost-code`), npm (if CLI wrapper), Homebrew formula, winget. |

---

## Immediate Next Steps (Post-PR #6)

These are the concrete tasks for the current branch `feat/phases-2-5-oauth-credential-store`:

✅ ~~1. [P2.1] CredentialStore trait + FileCredentialStore~~ → Done: PR #6
✅ ~~2. [P2.2] Credential enum (ApiKey + OAuth)~~ → Done: PR #6
✅ ~~3. [P2.3] Browser-based OAuth PKCE flow~~ → Done: PR #6
✅ ~~4. [P2.5] Token resolution from credential store~~ → Done: PR #6
✅ ~~5. [P2.6] ghost login <provider> command~~ → Done: PR #6
✅ ~~6. Multi-provider test (DeepSeek + Z.AI)~~ → Done: PR #6

⏳ **Next**: Address PR #6 review feedback
⏳ **[P2.4]** Device-code OAuth flow (headless)
⏳ **[P2.7]** `ghost logout <provider>` command
⏳ **[P2.8]** `ghost auth status` command
⏳ **[P1.8]** Runtime `/provider` slash command in TUI

---

## Deferred Work from PR #1 Review

From the [PR #1 review comment](https://github.com/ghostzali/ghost-prebuild/pull/1#issuecomment-5010613072):

> **One line for the roadmap**: when the EmbeddedProvider → ProviderConfig bridge is eventually built in Phase 1, parse the string into `ProviderAuthMode` via `serde_json::from_value` and **warn-on-unknown** rather than silently defaulting.

Tracked as **[P3.6]** above. The `auth_mode: Option<String>` field on `EmbeddedProvider` is captured but not yet consumed.

---

## Config Schema Target (matches shipped code from PR #1)

The `ProviderRegistry` deserializes from `[[providers]]` **array-of-tables** format.
The `ProviderAuthMode` enum has only `ApiKey | Codex` variants. OAuth is not yet implemented;
when Phase 2 adds it, the schema will migrate to a dual-arm `ProviderAuth` struct (see
Phase 1 migration note above).

```toml
# ~/.ghost/config.toml

# Default provider (used when no --provider flag)
default_provider = "openai"

# Configured providers override or extend the embedded catalog
# Uses [[providers]] array-of-tables syntax (matches shipped ProviderRegistry)

[[providers]]
name = "openai"
auth_mode = "api_key"
env_key = "OPENAI_API_KEY"
# api_base = "https://api.openai.com/v1"  # defaults from catalog

[[providers]]
name = "openai-codex"
auth_mode = "codex"
# API key from ~/.codex/auth.json (automatic)

[[providers]]
name = "custom-llm"
auth_mode = "api_key"
api_key = "sk-..."
api_base = "https://llm.mycompany.com/v1"
models = ["my-model-7b", "my-model-70b"]

# Illustrative Phase 2 target — not yet supported by the shipped schema:
# [[providers]]
# name = "anthropic"
# # Would need dual-arm ProviderAuth (api_key + oauth) — see Phase 1 migration note
# # Currently:
# #   auth_mode = "api_key"   → works (ANTHROPIC_API_KEY env)
# #   auth_mode = "oauth"     → not yet implemented (deserialization error)
# # Uses browser/device-code OAuth flow: ghost login anthropic --oauth
```
