# AGENTS.md

## Project mission
Build `zero-conf-mesh`, a Rust crate for zero-configuration LAN service discovery for multi-agent systems using mDNS/DNS-SD.

## Current repository state
This repository is now **scaffolded** and follows a **spec-first** approach.
- The main source of truth remains `docs/specs.md`.
- The crate scaffold (`Cargo.toml`, `src/`) is initialized.
- All code follows the repository specifications.

## Source of truth
When making decisions, use this order:
1. Explicit user instructions
2. `docs/specs.md`
3. This `AGENTS.md`
4. Conservative Rust best practices

If implementation must differ from the spec, update `docs/specs.md` in the same change or clearly document the mismatch.

## Expected technical direction
Unless the user says otherwise, prefer:
- Rust stable
- `tokio` for async runtime and background tasks
- `mdns-sd` for mDNS/DNS-SD
- `Arc<RwLock<...>>` or channels only where they keep the API simple
- `serde` for serializable metadata/types when useful
- small, composable modules instead of one large file

## Design principles
- Keep the public API minimal, ergonomic, and async-first.
- Prioritize correctness and observability over cleverness.
- Favor explicit types for agent status, events, config, and errors.
- Make crash recovery and stale-node eviction first-class concerns.
- Design for local-network operation only; do not imply cross-subnet guarantees.
- Keep metadata payloads compact and predictable.

## Implementation guidance
When scaffolding the crate, prefer a layout close to:
- `src/lib.rs`
- `src/builder.rs`
- `src/config.rs`
- `src/error.rs`
- `src/registry.rs`
- `src/broadcaster.rs`
- `src/listener.rs`
- `src/types.rs`
- `tests/`

If the implementation naturally suggests a better layout, that is fine—keep module boundaries clear.

## Testing expectations
For any non-trivial code change, aim to cover:
- unit tests for registry behavior and TTL eviction
- parsing/serialization tests for metadata and TXT records
- builder/config validation tests
- integration tests for join/leave/discovery flows when practical

Avoid flaky tests that depend on fragile host-network assumptions unless explicitly requested.

## Working rules for agents
- Make small, reviewable changes following **Conventional Commits** (e.g., `feat:`, `fix:`, `chore:`).
- Do not invent features outside the scope of `docs/specs.md` unless asked.
- Preserve backward-compatible API evolution where possible.
- Add or update docs when behavior changes.
- Prefer `cargo fmt`, `cargo clippy`, and `cargo test` once the crate exists.
- If a command cannot run because the crate has not been scaffolded yet, say so plainly.

## Definition of done
A change is in good shape when it:
- matches the spec,
- is documented,
- is formatted,
- is lint-clean,
- and has appropriate tests for the level of change.

<!-- MES:START -->
## mes agent workflow

This repository is configured to use `mes` for local LAN agent discovery.

If `.mes.toml` is missing on this machine, run `mes init --force` before using the commands below.

Recommended commands for AI agents in this repo:
- bring this repo's agent online: `mes up`
- list peers for this project: `mes who --config .mes.toml --project zero-conf-mesh`
- find a reviewer quickly: `mes who --config .mes.toml --project zero-conf-mesh --role reviewer`
- mirror live mesh state to a file: `mes watch --config .mes.toml --write-state /tmp/zero-conf-mesh-mes-state.json`
- start the local HTTP + SSE bridge: `mes serve --config .mes.toml --bind 127.0.0.1:9999`

The generated config already includes this repo's defaults for project, branch, ports, and discovery settings.
Prefer reusing a single long-running `mes up` process instead of starting multiple announcers for the same machine.
<!-- MES:END -->
