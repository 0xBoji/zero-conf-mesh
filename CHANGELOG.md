# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Detailed, onboarding-focused README with architecture, workflows, and failure-mode explanations.

## [0.1.0] - 2026-04-02

### Added
- Initial `zero-conf-mesh` crate scaffold and spec-first project structure.
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
