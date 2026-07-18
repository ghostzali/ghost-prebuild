<div align="center">

<h1>
  <picture>
    <!-- Logo assets coming soon — PRs welcome! -->
    <img alt="Ghost Prebuild logo" width="96">
  </picture>
  <br>
  Ghost Prebuild (<code>ghost</code>)
</h1>

**Ghost Prebuild** is a terminal-based AI coding agent, forked from the open-source 
Grok Build CLI. It runs as a full-screen TUI that understands your codebase, edits files, 
executes shell commands, searches the web, and manages long-running tasks — interactively,
headlessly for scripting/CI, or embedded in editors via the Agent Client Protocol (ACP).

**Key differentiators from Grok Build:**
- 🔌 **OpenAI-compatible API providers** — Works with any OpenAI-compatible endpoint 
  (OpenAI, xAI, Anthropic, local Ollama, LiteLLM proxies, etc.)
- 🔑 **Multiple auth profiles** — Configure multiple API keys and switch between them 
  easily at runtime
- 🎛️ **Provider-agnostic models** — Map any model from any provider with full 
  context-window and parameter customization

[Installing the released binary](#installing-the-released-binary) ·
[Building from source](#building-from-source) ·
[Multi-Provider Setup](#multi-provider-setup) ·
[Documentation](#documentation) ·
[Repository layout](#repository-layout) ·
[Development](#development) ·
[License](#license)

</div>

---

## Installing the released binary

Prebuilt binaries are published for macOS, Linux, and Windows:

```sh
# Install scripts coming soon — currently build from source
# curl -fsSL https://ghost-prebuild.dev/install.sh | bash   # macOS / Linux
# irm https://ghost-prebuild.dev/install.ps1 | iex          # Windows PowerShell
```

## Building from source

Requirements:

- **Rust** — the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` installs it automatically on first build.
- **[DotSlash](https://dotslash-cli.com)** — required so hermetic tools under
  [`bin/`](bin/) (notably [`bin/protoc`](bin/protoc)) can download and run.
  Install it and ensure `dotslash` is on your `PATH` **before** building:

  ```sh
  cargo install dotslash
  /usr/bin/env dotslash --help   # sanity check
  ```

- **protoc** — proto codegen resolves [`bin/protoc`](bin/protoc) via DotSlash,
  or falls back to a `protoc` on `PATH` / `$PROTOC`.
- macOS and Linux are supported build hosts; Windows builds are best-effort
  and not currently tested from this tree.

```sh
cargo run -p xai-grok-pager-bin              # build + launch the TUI
cargo build -p xai-grok-pager-bin --release  # release binary: target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin            # fast validation
```

The binary artifact is named `xai-grok-pager`; official installs ship it as
`ghost`.

## Multi-Provider Setup

Ghost Prebuild supports multiple OpenAI-compatible API providers simultaneously. 
Configure them in `~/.ghost/config.toml`:

```toml
# Default provider used when no --provider flag is given
default_provider = "openai"

[[providers]]
name = "openai"
api_base = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"          # resolved from env var
models = ["gpt-4o", "gpt-4.1", "o4-mini", "o3-mini"]

[[providers]]
name = "xai"
api_base = "https://api.x.ai/v1"
api_key = "${XAI_API_KEY}"
models = ["grok-4", "grok-4.1", "grok-3"]

[[providers]]
name = "anthropic"
api_base = "https://api.anthropic.com/v1"  # or use a proxy like LiteLLM
api_key = "${ANTHROPIC_API_KEY}"
models = ["claude-sonnet-4-20250514", "claude-opus-4-20250514"]

[[providers]]
name = "local"
api_base = "http://localhost:11434/v1"     # Ollama
api_key = "ollama"                         # Ollama ignores the key
models = ["llama3.3:70b", "qwen3:32b", "codestral:22b"]
```

**Codex subscription (zero-config):**
If you already have [Codex CLI](https://github.com/openai/codex) installed and logged in,
Ghost Prebuild auto-detects your ChatGPT subscription and makes it available as the `codex` provider.

> **Note on token expiry**: Codex OAuth access tokens are short-lived (~1 hour). Ghost Prebuild
> reads the token fresh from `~/.codex/auth.json` on each request; the Codex CLI background
> process refreshes this file periodically. If you get 401 errors, run `codex login` to force
> a token refresh, then restart ghost.

```sh
# No config needed — auto-detected from ~/.codex/auth.json
ghost --provider codex --model gpt-5.6-sol
```

The `codex` provider reads your OAuth tokens from `~/.codex/auth.json` (the same
file Codex uses), so there's no separate login step. Supported models include all
the models your ChatGPT subscription grants access to.

**Switching providers at runtime:**
```sh
ghost --provider openai                          # use OpenAI API key
ghost --provider codex                           # use ChatGPT subscription
ghost --provider xai --model grok-4              # use xAI's grok-4
ghost --provider local --model codestral:22b     # use local Ollama model
```

**Environment variables:**
```sh
export GHOST_DEFAULT_PROVIDER="openai"
export GHOST_PROVIDER_OPENAI_API_KEY="sk-..."
export GHOST_PROVIDER_XAI_API_KEY="..."
```

Ghost Prebuild also maintains backward compatibility with the `GROK_*` and 
`XAI_*` environment variables from Grok Build, but `GHOST_*` variables take 
precedence.

> **Note on model catalogs**: Models can be defined in two places — the embedded
> `default_models.json` provider catalog (metadata-rich: name, description,
> context_window) and the `config.toml` provider `models` list (simple IDs).
> The config.toml `models` list acts as a **filter/override**: if set, only those
> models are available from that provider, regardless of the embedded catalog.
> If empty or unset, the embedded catalog is used as the full model list.

## Documentation

Documentation ships with the pager crate:
[`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)
— getting started, keyboard shortcuts, slash commands, configuration, theming,
MCP servers, skills, plugins, hooks, headless mode, sandboxing, and more.

## Repository layout

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-pager-bin` | Composition-root package; builds the `xai-grok-pager` binary |
| `crates/codegen/xai-grok-pager` | The TUI: scrollback, prompt, modals, rendering |
| `crates/codegen/xai-grok-shell` | Agent runtime + leader/stdio/headless entry points |
| `crates/codegen/xai-grok-tools` | Tool implementations (terminal, file edit, search, ...) |
| `crates/codegen/xai-grok-workspace` | Host filesystem, VCS, execution, checkpoints |
| `crates/codegen/...` | The rest of the CLI crate closure (config, MCP, markdown, sandbox, ...) |
| `crates/common/`, `crates/build/`, `prod/mc/` | Small shared leaf crates pulled in by the closure |
| `third_party/` | Vendored upstream source (Mermaid diagram stack) |

> [!IMPORTANT]
> The root `Cargo.toml` (workspace members, dependency versions, lints,
> profiles) is **generated** — treat it as read-only. Prefer editing per-crate
> `Cargo.toml` files.

## Development

```sh
cargo check -p <crate>        # always target specific crates; full-workspace builds are slow
cargo test -p xai-grok-config # per-crate tests
cargo clippy -p <crate>       # lint config: clippy.toml at the repo root
cargo fmt --all               # rustfmt.toml at the repo root
```

## Contributing

Contributions are welcome! This is a fork of the Grok Build open-source project, 
rebranded and extended for multi-provider compatibility.

## License

First-party code in this repository is licensed under the **Apache License,
Version 2.0** — see [`LICENSE`](LICENSE).

Third-party and vendored code remains under its original licenses. See:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git dependencies,
  bundled UI themes, and **in-tree source ports** (including openai/codex and
  sst/opencode tool implementations)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
  — crate-local notice for the codex and opencode ports (license texts +
  Apache §4(b) change notice)
- [`third_party/NOTICE`](third_party/NOTICE) — vendored Mermaid-stack index
