# Technical Specification: `zero-conf-mesh`

## 1. Overview
`zero-conf-mesh` is a Rust library for zero-configuration LAN service discovery for multi-agent systems using mDNS and DNS-SD. It is designed for local-network coordination only: agents advertise themselves, discover peers, and maintain an in-memory registry of currently known nodes without requiring any central service.

The crate currently provides:
- a typed async-first builder and runtime handle,
- mDNS/DNS-SD advertisement for the local node,
- background browsing for remote peers,
- a concurrent in-memory registry with TTL-based stale-node eviction,
- lifecycle events for join, update, and leave transitions,
- and a first-party CLI binary (`mes`) for shell/LLM-driven interaction, including repo-local bootstrap via `mes init`.

At the MVP level, every node should advertise enough data for peers to identify:
- who the node is (`agent_id`),
- which project it is currently attached to (`current_project`),
- which branch or workstream it is currently attached to (`current_branch`).

## 2. Goals and Non-Goals

### Goals
- Zero manual peer configuration on a shared LAN.
- Small, ergonomic, async-first Rust API.
- Compact, predictable TXT metadata.
- Explicit lifecycle events for observability.
- Graceful shutdown plus crash-tolerant stale-node eviction.

### Non-Goals
- Cross-subnet or WAN discovery.
- Strong delivery guarantees.
- Full PKI-style identity, encryption, or WAN-safe trust guarantees.
- Consensus, leader election, or distributed locking.

## 3. Architecture
The runtime is composed of four cooperating parts:

1. **Builder / Config**: validates static settings such as service type, ports, TTL, and heartbeat cadence.
2. **Broadcaster**: registers and re-announces the local service via `mdns-sd`.
3. **Listener**: browses the configured service type and converts resolved peers into typed announcements.
4. **Registry**: stores discovered agents and emits lifecycle events.
5. **CLI (`mes`)**: optional text/JSON interface for agents that prefer shell commands over direct Rust integration, including observer lookups, event watching, state-file export, shell completion generation, and a local REST/SSE bridge.

### 3.1 Concurrency Model
The crate uses `tokio` for orchestration and `mdns-sd` for network I/O.

- `ZeroConfMesh` owns the runtime.
- The local advertisement is stored in `Arc<RwLock<AgentAnnouncement>>` so it can be updated safely at runtime.
- The registry is a cloneable `Arc<RwLock<HashMap<...>>>` wrapper.
- Background tasks are spawned for:
  - periodic heartbeat re-announcement,
  - periodic TTL sweeping,
  - continuous mDNS browse event consumption.
- shutdown signaling uses `tokio::sync::watch`.
- registry event fan-out uses `tokio::sync::broadcast`.

### 3.2 Component Diagram
```text
                    +-------------------+
                    |   ZeroConfMesh    |
                    |  config + tasks   |
                    +---------+---------+
                              |
          +-------------------+-------------------+
          |                   |                   |
          v                   v                   v
+----------------+   +----------------+   +----------------+
|  Broadcaster   |   |    Listener    |   |    Registry    |
| register/update|   | browse/resolve |   | in-memory view |
+--------+-------+   +--------+-------+   +--------+-------+
         |                    |                    |
         v                    v                    v
     mdns-sd daemon <---- local network ----> lifecycle events
```

### 3.3 Runtime Flow
1. Build validated config.
2. Create `mdns-sd` daemon.
3. Construct local announcement and register it.
4. Insert the local node into the registry as a local-origin entry.
5. Start listener, heartbeat, and sweeper tasks.
6. Remote `ServiceResolved` events are parsed into `AgentAnnouncement` values, optionally verified against a shared secret, and then upserted into the registry.
7. Remote `ServiceRemoved` events remove matching peers by instance name.
8. On shutdown, the local service is unregistered, local registry state is removed, tasks stop, and the daemon is shut down.

For observer-style use cases, the runtime also supports discovery-only mode where:
- the listener and sweeper run,
- the local node is not advertised,
- the local node is not inserted into the registry.

The `mes` CLI builds on this mode for `list`, `get`, and `watch` commands, and can optionally mirror the current registry to a JSON file for file-oriented agents.
It also supports:
- `init` for creating `.mes.toml` plus repository-specific AGENTS guidance,
- `up` for announcing directly from the generated config,
- `who` as a human/agent-friendly alias for `list`,
- append-only JSONL event export,
- shell-command hooks fed by JSON over stdin,
- a local REST bridge for non-shell agent runtimes,
- an SSE event stream for realtime agent observers,
- shell completion generation.

## 4. Data Model

### 4.1 Network Identity
- **Service Type**: `_agent-mesh._tcp.local.` by default.
- **Instance Name**: `{agent_id}.{service_type}`.
- **Host Name**: `{agent_id}.local.`.

### 4.2 TXT Metadata
The canonical TXT keys are:
- `agent_id`
- `role`
- `current_project`
- `current_branch`
- `status`
- `capabilities`

When shared-secret auth is enabled, the crate also emits reserved auth metadata:
- `zcm_auth`
- `zcm_sig`

The Vietnamese MVP spec requires `agent_id`, `current_project`, and `current_branch` as first-class discovery fields. The current implementation also keeps `role` and `status` so the mesh can answer “who is doing what?” rather than only “who exists?”.

Additional metadata is allowed as arbitrary string key/value pairs.

Constraints:
- keys must be non-empty,
- required keys must be present when parsing remote peers,
- values are expected to be valid UTF-8,
- payloads should remain compact; keeping total TXT data under roughly 400 bytes is recommended.

### 4.3 `AgentStatus`
```rust
pub enum AgentStatus {
    Idle,
    Busy,
    Error,
}
```

Properties:
- serialized with snake_case strings,
- parseable from TXT metadata,
- currently limited to `idle`, `busy`, and `error`.

### 4.4 `AgentAnnouncement`
`AgentAnnouncement` is the wire-adjacent structure used to:
- build TXT properties,
- construct `mdns_sd::ServiceInfo`,
- parse resolved services from `mdns-sd`,
- convert into registry state.

Important fields:
- `instance_name: String`
- `agent_id: String`
- `role: String`
- `project: String`
- `branch: String`
- `status: AgentStatus`
- `capabilities: Vec<String>`
- `port: u16`
- `addresses: Vec<IpAddr>`
- `metadata: AgentMetadata`

### 4.5 `AgentInfo`
`AgentInfo` is the in-memory registry representation of a discovered agent.

```rust
pub struct AgentInfo {
    instance_name: String,
    id: String,
    role: String,
    project: String,
    branch: String,
    status: AgentStatus,
    capabilities: Vec<String>,
    port: u16,
    addresses: Vec<IpAddr>,
    metadata: AgentMetadata,
    last_seen: Instant,
}
```

Notes:
- `last_seen` is monotonic and used only for runtime TTL management.
- `AgentInfo` is cloned for snapshots and event emission.
- serialization is intentionally not implemented for `AgentInfo` because `Instant` is process-local and not portable across processes.

## 5. Public API

### 5.1 Main Entry Points
```rust
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("reviewer")
    .project("alpha")
    .branch("main")
    .port(8080)
    .build()
    .await?;

mesh.update_status(AgentStatus::Busy).await?;
mesh.update_project("beta").await?;
mesh.update_branch("feature/runtime").await?;
mesh.update_metadata("capability", "planning").await?;
mesh.remove_metadata("capability").await?;

let local = mesh.local_agent().await;
let all_agents = mesh.agents().await;
let alpha_agents = mesh.agents_by_project("alpha").await;
let maybe_peer = mesh.get_agent("agent-02").await;

let mut events = mesh.subscribe();
```

### 5.2 `ZeroConfMeshBuilder`
Current builder setters:
- `agent_id(...)`
- `role(...)`
- `project(...)`
- `branch(...)`
- `port(...)`
- `mdns_port(...)`
- `service_type(...)`
- `status(...)`
- `heartbeat_interval(...)`
- `ttl(...)`
- `event_capacity(...)`
- `capability(...)`
- `capabilities(...)`
- `advertise_local(...)`
- `discover_only()`
- `enable_interface(...)`
- `disable_interface(...)`
- `shared_secret(...)`
- `shared_secret_with_mode(...)`
- `shared_secret_rotation(...)`
- `shared_secret_rotation_with_mode(...)`
- `metadata(key, value)`
- `metadata_map(...)`
- `build().await`

Defaults:
- random UUID `agent_id` if omitted,
- role = `agent`,
- project = `default`,
- branch = `unknown`,
- service type = `_agent-mesh._tcp.local.`,
- mDNS port = `5353`,
- heartbeat = `30s`,
- TTL = `120s`,
- event capacity = `256`,
- initial status = `Idle`.

### 5.3 High-Level Runtime API
`ZeroConfMesh` currently exposes:
- `builder()`
- `from_config(...).await`
- `config()`
- `registry()`
- `local_agent_id()`
- `local_agent().await`
- `get_agent(...).await`
- `agents().await`
- `agents_by_project(...).await`
- `agents_by_branch(...).await`
- `agents_by_project_and_branch(...).await`
- `agents_by_status(...).await`
- `agents_by_role(...).await`
- `agents_with_metadata_key(...).await`
- `agents_with_metadata(...).await`
- `agents_with_metadata_key_prefix(...).await`
- `agents_with_metadata_prefix(...).await`
- `agents_with_metadata_regex(...).await`
- `agents_with_capability(...).await`
- `query_agents(...).await`
- `who_is_on_branch(...).await`
- `subscribe()`
- `update_status(...).await`
- `update_project(...).await`
- `update_branch(...).await`
- `update_metadata(...).await`
- `remove_metadata(...).await`
- `update_capabilities(...).await`
- `add_capability(...).await`
- `remove_capability(...).await`
- `shutdown().await`

`registry()` is still available for advanced read access, but typical consumers should prefer the high-level query methods on `ZeroConfMesh`.

`update_metadata(...)` is intended for non-canonical extension keys only. Canonical keys such as `agent_id`, `current_project`, `current_branch`, `role`, `status`, `capabilities`, `zcm_auth`, and `zcm_sig` remain managed by the crate so callers do not accidentally create divergent runtime state.

### 5.4 Lifecycle Events
```rust
pub enum EventOrigin {
    Local,
    Remote,
}

pub enum DepartureReason {
    Graceful,
    Expired,
}

pub enum AgentEvent {
    Joined { agent, origin },
    Updated { previous, current, origin },
    Left { agent, origin, reason },
}
```

Semantics:
- local inserts/updates/removals emit `origin: Local`,
- listener-driven peer discovery emits `origin: Remote`,
- TTL eviction emits `reason: Expired`,
- explicit unregister/removal emits `reason: Graceful`.

### 5.5 Error Model
`ZeroConfError` currently covers:
- empty required fields,
- invalid service type,
- invalid advertised port,
- invalid mDNS daemon port,
- invalid event channel capacity,
- invalid heartbeat/TTL relationship,
- empty metadata keys,
- empty shared secrets,
- invalid capability names,
- invalid metadata regex patterns,
- reserved canonical metadata keys used with generic metadata updaters,
- missing required TXT properties,
- missing authentication metadata for verified peers,
- invalid authentication scheme metadata,
- invalid TXT property encoding,
- invalid status strings,
- invalid shared-secret signatures,
- wrapped `mdns_sd::Error`,
- background task join errors.

## 6. Registry and Eviction Semantics

### 6.1 Upsert Behavior
Registry updates produce one of:
- `Inserted`
- `Updated`
- `Refreshed`

Rules:
- same agent ID + different payload => `Updated`,
- same payload + newer observation time => `Refreshed`,
- unknown agent ID => `Inserted`.

### 6.2 TTL Sweep
The sweeper runs on the configured heartbeat interval and removes peers whose:

```text
now - last_seen > ttl
```

Recommended ratio:
- heartbeat around one quarter of TTL,
- default implementation uses `30s` heartbeat and `120s` TTL.

This ratio provides:
- several opportunities to refresh before eviction,
- tolerance for transient packet loss,
- bounded cleanup latency.

### 6.3 Graceful vs Crash Leave
- **Graceful shutdown**: listener should observe a removal and the registry emits `Left { reason: Graceful }`.
- **Crash / abrupt exit**: no goodbye is guaranteed; stale peers are eventually evicted with `Left { reason: Expired }`.

## 7. Constraints and Edge Cases
- **Local-network only**: mDNS does not cross routers/subnets by default.
- **Multiple interfaces**: interface selection is delegated to `mdns-sd`, but the current implementation exposes explicit include/exclude interface selectors through the builder/config in addition to custom mDNS UDP port support for test isolation.
- **Local address population**: local `ServiceInfo` enables `addr_auto` so the daemon can fill host addresses automatically.
- **Shared-secret verification**: when enabled in `SignAndVerify` mode, unsigned or invalidly signed peers are ignored by the listener.
- **TXT parsing**: remote peers missing required fields are ignored rather than partially inserted.
- **Dropped event subscribers**: registry event delivery is best-effort through a broadcast channel.
- **Duplicate announcements**: repeated identical payloads only refresh `last_seen`; they do not emit update events.

## 8. Crate Dependencies
- **`tokio`**: async runtime, tasks, `watch`, `broadcast`, and timing primitives.
- **`mdns-sd`**: mDNS/DNS-SD registration, browsing, and service resolution.
- **`serde`**: serialization for portable wire-friendly types such as `AgentStatus` and `AgentAnnouncement`.
- **`uuid`**: default agent ID generation.
- **`thiserror`**: typed library error definitions.
- **`tracing`**: lightweight observability for runtime tasks and daemon interaction.
- **`clap`**: first-party CLI argument parsing for the `mes` binary.
- **`serde_json`**: JSON output for shell- and agent-friendly CLI responses.
- **`toml`**: repo-local CLI config parsing and generation for `.mes.toml`.

## 9. Testing Strategy
The current testing strategy is split into three layers.

### 9.1 Unit Tests
Cover:
- builder validation,
- config validation,
- status parsing,
- TXT conversion and parsing,
- registry insert/update/refresh behavior,
- registry removal by instance name,
- TTL eviction behavior,
- event origin/reason semantics,
- local-vs-remote registry update semantics.

### 9.2 Runtime Tests
Use a custom mDNS UDP port to avoid depending on the host's system Bonjour/Avahi setup.

Scenarios:
- local mesh creation and shutdown,
- local status update propagation,
- local project/branch/metadata update propagation,
- local metadata removal propagation,
- local typed capability update propagation,
- shared-secret verified discovery between signed peers,
- unsigned peers being ignored when verification is enabled,
- rotated shared secrets being accepted during key transition windows,
- discovery between two mesh nodes on the same custom mDNS port,
- remote peer status update propagation after a local status change,
- remote peer project/branch/metadata update propagation after a local runtime change,
- remote peer metadata removal propagation after a local runtime change,
- remote peer capability propagation plus advanced metadata query coverage,
- multi-peer discovery on the same custom mDNS port,
- project isolation via query helpers on a shared LAN,
- malformed remote TXT payloads being ignored by the listener.
- startup failure cleanup ensuring partially announced local services do not remain discoverable.

### 9.3 Failure Simulation
Crash-like behavior is simulated by inserting peers with stale `last_seen` timestamps and running eviction logic directly. This avoids flaky host-network assumptions while still validating stale-node cleanup behavior.

### 9.4 Documentation and Example Tests
The crate should treat docs as part of the test surface:
- rustdoc examples on the crate root and primary public types should compile,
- examples in `examples/` should stay buildable under `cargo test`,
- README snippets should match the current public API.
- CLI helper tests should cover parsers for metadata and interface selectors.
- CLI state-file helpers should preserve atomic JSON snapshots when possible.

In practice, the project should keep passing:
- `cargo test`
- `cargo test --doc`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo package --locked`

### 9.5 Tests That Should Be Added Next
The current suite is good for the present slice, but the next implementation steps should add:
- broadcaster tests for repeated re-registration after local status changes,
- event-subscriber lag/drop tests around the broadcast channel's best-effort semantics.

### 9.6 Test Design Rules
To keep the suite reliable:
- prefer deterministic unit tests over host-network-dependent integration tests,
- use custom mDNS ports for runtime tests to reduce interference from system Bonjour/Avahi services,
- avoid assertions that depend on exact packet timing,
- validate observable outcomes (`registry`, events, status transitions) rather than daemon internals,
- keep tests small and composable so failures point to a single behavior.

## 10. Future Work
- richer status vocabularies or user-defined states,
- stronger authentication options such as asymmetric signatures,
- encrypted metadata payloads,
- leader election or higher-level coordination protocols,
- richer authorization policies on top of shared-secret verification.
