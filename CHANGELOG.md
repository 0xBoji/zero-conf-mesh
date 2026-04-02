# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.6](https://github.com/0xBoji/coding_agent_mesh_presence/compare/v0.1.5...v0.1.6) - 2026-04-02

### Other

- add curl-based camp installer
- add bash install commands for the renamed crate

## [0.1.5](https://github.com/0xBoji/coding_agent_mesh_presence/compare/v0.1.4...v0.1.5) - 2026-04-02
### Added

- `camp init` to generate a repo-local `.camp.toml` file and inject project-specific `camp` usage guidance into `AGENTS.md`.
- `camp up` to announce a local agent directly from the generated config.
- Optional `--config` support for observer-style CLI commands so `camp who`, `camp watch`, and `camp serve` can reuse the initialized discovery profile.

### Changed

- `.camp.toml` and `camp.toml` are now ignored by git so machine-local mesh identities stay local.
- bootstrap the first manual publish under the renamed `coding_agent_mesh_presence` crate before handing release flow back to `release-plz`.

## [0.1.4](https://github.com/0xBoji/coding_agent_mesh_presence/compare/v0.1.3...v0.1.4) - 2026-04-02

### Added

- stream live events from camp serve
- add local rest bridge to camp
- add exec hooks to camp watch

## [0.1.3](https://github.com/0xBoji/coding_agent_mesh_presence/compare/v0.1.2...v0.1.3) - 2026-04-02

### Added

- expand camp agent workflow commands
- add file-based state export to camp
- add camp cli for shell-driven discovery
- support shared secret rotation
- add authenticated LAN discovery controls

### Added
- Advanced metadata queries for key-prefix, value-prefix, regex, and custom-predicate filtering.
- Typed `capabilities` support in announcements, runtime updates, and peer queries.
- Explicit interface include/exclude controls for the embedded `mdns-sd` daemon.
- Optional shared-secret signing and peer verification modes for authenticated LAN discovery.
- Shared-secret rotation support so new nodes can verify peers signed with previous secrets during rollout.
- First-party `camp` CLI with JSON-friendly `announce`, `list`, `get`, and `watch` commands for shell/LLM-driven agents.
- Discovery-only runtime mode so observer/query processes do not announce themselves on the LAN.
- `camp watch --write-state ...` support for file-based JSON snapshots aimed at simple shell/LLM agents.
- `camp who` alias, `camp watch --write-events ...` JSONL logging, and `camp completions ...` generation.
- `camp watch --exec ...` hook execution with JSON piped to stdin for reactive agent workflows.
- `camp serve` local REST bridge for Python/TypeScript agent runtimes.
- `camp serve /events` SSE stream for live mesh snapshots and lifecycle updates.

### Changed
- `capabilities` is now treated as a canonical first-class presence field instead of only ad-hoc metadata.

### Tested
- Advanced metadata query coverage in unit and runtime tests.
- Typed capability propagation across multi-node discovery.
- Builder/config handling for interface selection controls.
- Shared-secret verified discovery and rejection of unsigned peers when verification is enabled.
- Rotated shared-secret acceptance during transition windows.
- CLI parser coverage for metadata and interface selectors.
- CLI state-file path helper coverage.
- CLI JSONL event-log helper coverage.
- CLI exec-hook stdin coverage.

## [0.1.2](https://github.com/0xBoji/coding_agent_mesh_presence/compare/v0.1.1...v0.1.2) - 2026-04-02

### Other

- add crates.io badge to README

## [0.1.1](https://github.com/0xBoji/coding_agent_mesh_presence/compare/v0.1.0...v0.1.1) - 2026-04-02

### Other

- add ai keyword to crates metadata
- add crates metadata keywords and categories
- add automated crates release workflow

### Added
- Detailed, onboarding-focused README with architecture, workflows, and failure-mode explanations.
- Configurable lifecycle event channel capacity via builder/config.
- Query helpers for filtering peers by role and metadata.
- Runtime metadata removal API for non-canonical metadata keys.

## [0.1.0] - 2026-04-02

### Added
- Initial `coding_agent_mesh_presence` crate scaffold and spec-first project structure.
- Async `ZeroConfMesh` runtime with builder-driven startup.
- Local mDNS/DNS-SD advertisement using `mdns-sd`.
- Background LAN peer discovery and resolved-service parsing.
- Concurrent in-memory registry with:
  - insert/update/refresh semantics,
  - project/branch/status filtering,
  - lifecycle event fan-out,
  - TTL-based stale peer eviction.
- Typed public models and events:
  - `AgentAnnouncement`,
  - `AgentInfo`,
  - `AgentStatus`,
  - `AgentEvent`,
  - `EventOrigin`,
  - `DepartureReason`.
- Branch-aware metadata support via `current_branch`.
- Runtime update APIs for:
  - status,
  - project,
  - branch,
  - extra non-canonical metadata.
- Examples:
  - `single_node`,
  - `two_nodes`.
- GitHub Actions CI for formatting, linting, tests, doctests, and package verification.
- Explicit package include list for cleaner publishing artifacts.

### Changed
- Startup flow now cleans up local registration if initialization fails after announcing but before listener startup succeeds.
- Package contents are constrained so local tooling/session files are not shipped in crate archives.
- README and specs were expanded to match the current implementation and testing surface.

### Fixed
- Listener ignores malformed remote TXT payloads instead of partially inserting invalid peers.
- Startup failure path no longer leaves a partially announced service discoverable on the network.

### Tested
- Builder/config validation.
- TXT conversion and parsing.
- Registry insert/update/refresh semantics.
- Removal by instance name.
- TTL eviction behavior.
- Event origin/reason behavior.
- Two-node and multi-peer discovery on custom mDNS ports.
- Remote propagation of status/project/branch/metadata updates.
- Project isolation on a shared LAN.
- Malformed TXT listener handling.
- Startup failure cleanup behavior.
