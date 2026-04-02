# Technical Specification: `zero-conf-mesh`

## 1. Overview
`zero-conf-mesh` is a Rust library for zero-configuration LAN service discovery for multi-agent systems using mDNS and DNS-SD. It is designed for local-network coordination only: agents advertise themselves, discover peers, and maintain an in-memory registry of currently known nodes without requiring any central service.

The crate currently provides:
- a typed async-first builder and runtime handle,
- mDNS/DNS-SD advertisement for the local node,
- background browsing for remote peers,
- a concurrent in-memory registry with TTL-based stale-node eviction,
- lifecycle events for join, update, and leave transitions.

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
- Authentication or encryption of advertisements.
- Consensus, leader election, or distributed locking.

## 3. Architecture
The runtime is composed of four cooperating parts:

1. **Builder / Config**: validates static settings such as service type, ports, TTL, and heartbeat cadence.
2. **Broadcaster**: registers and re-announces the local service via `mdns-sd`.
3. **Listener**: browses the configured service type and converts resolved peers into typed announcements.
4. **Registry**: stores discovered agents and emits lifecycle events.

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
6. Remote `ServiceResolved` events are parsed into `AgentAnnouncement` values and upserted into the registry.
7. Remote `ServiceRemoved` events remove matching peers by instance name.
8. On shutdown, the local service is unregistered, local registry state is removed, tasks stop, and the daemon is shut down.

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
- `who_is_on_branch(...).await`
- `subscribe()`
- `update_status(...).await`
- `shutdown().await`

`registry()` is still available for advanced read access, but typical consumers should prefer the high-level query methods on `ZeroConfMesh`.

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
- invalid heartbeat/TTL relationship,
- empty metadata keys,
- missing required TXT properties,
- invalid TXT property encoding,
- invalid status strings,
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
- **Multiple interfaces**: interface selection is delegated to `mdns-sd`; the current implementation also supports a custom mDNS UDP port for test isolation.
- **Local address population**: local `ServiceInfo` enables `addr_auto` so the daemon can fill host addresses automatically.
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
- discovery between two mesh nodes on the same custom mDNS port,
- remote peer status update propagation after a local status change,
- multi-peer discovery on the same custom mDNS port,
- project isolation via query helpers on a shared LAN.

### 9.3 Failure Simulation
Crash-like behavior is simulated by inserting peers with stale `last_seen` timestamps and running eviction logic directly. This avoids flaky host-network assumptions while still validating stale-node cleanup behavior.

### 9.4 Documentation and Example Tests
The crate should treat docs as part of the test surface:
- rustdoc examples on the crate root and primary public types should compile,
- examples in `examples/` should stay buildable under `cargo test`,
- README snippets should match the current public API.

In practice, the project should keep passing:
- `cargo test`
- `cargo test --doc`
- `cargo clippy --all-targets --all-features -- -D warnings`

### 9.5 Tests That Should Be Added Next
The current suite is good for the present slice, but the next implementation steps should add:
- listener tests for malformed remote TXT payloads being ignored,
- broadcaster tests for repeated re-registration after local status changes,
- failure-path startup tests asserting partially initialized runtimes clean up local registration,
- event-subscriber lag/drop tests around the broadcast channel's best-effort semantics.

### 9.6 Test Design Rules
To keep the suite reliable:
- prefer deterministic unit tests over host-network-dependent integration tests,
- use custom mDNS ports for runtime tests to reduce interference from system Bonjour/Avahi services,
- avoid assertions that depend on exact packet timing,
- validate observable outcomes (`registry`, events, status transitions) rather than daemon internals,
- keep tests small and composable so failures point to a single behavior.

## 10. Future Work
- configurable event channel capacity,
- richer status vocabularies or user-defined states,
- optional filtering helpers for roles/capabilities,
- encrypted or signed metadata payloads,
- leader election or higher-level coordination protocols,
- more explicit network-interface controls if required by real deployments.
