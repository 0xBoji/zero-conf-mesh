# AGENTS.md

## Project mission
Build `zero-conf-mesh`, a Rust crate for zero-configuration LAN service discovery for multi-agent systems using mDNS/DNS-SD.

## Current repository state
This repository is currently **spec-first**.
- The main source of truth is `docs/specs.md`.
- There is **no crate scaffold yet** (`Cargo.toml`, `src/`, and tests are not present yet).
- Before implementing code, read and align with `docs/specs.md`.

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
- Make small, reviewable changes.
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

<!-- OMX:RUNTIME:START -->
<session_context>
**Session:** omx-1775071361973-otwpac | 2026-04-01T19:22:41.996Z

**Explore Command Preference:** enabled via `USE_OMX_EXPLORE_CMD` (default-on; opt out with `0`, `false`, `no`, or `off`)
- Advisory steering only: agents SHOULD treat `omx explore` as the default first stop for direct inspection and SHOULD reserve `omx sparkshell` for qualifying read-only shell-native tasks.
- For simple file/symbol lookups, use `omx explore` FIRST before attempting full code analysis.
- When the user asks for a simple read-only exploration task (file/symbol/pattern/relationship lookup), strongly prefer `omx explore` as the default surface.
- Explore examples: `omx explore...

**Compaction Protocol:**
Before context compaction, preserve critical state:
1. Write progress checkpoint via state_write MCP tool
2. Save key decisions to notepad via notepad_write_working
3. If context is >80% full, proactively checkpoint state
</session_context>
<!-- OMX:RUNTIME:END -->
