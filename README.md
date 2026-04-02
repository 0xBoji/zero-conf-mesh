# zero-conf-mesh

Zero-configuration LAN service discovery for multi-agent systems in Rust using mDNS/DNS-SD.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](#installation)
[![CI](https://github.com/0xBoji/zero-conf-mesh/actions/workflows/ci.yml/badge.svg)](https://github.com/0xBoji/zero-conf-mesh/actions/workflows/ci.yml)

> Think of `zero-conf-mesh` as a small local-network presence layer for agents and tools:
> each node advertises who it is, what project/branch it belongs to, and what it is doing right now,
> while every peer keeps a live in-memory registry of the LAN.

---

## Table of Contents

- [What this crate is](#what-this-crate-is)
- [Why it exists](#why-it-exists)
- [Who should use it](#who-should-use-it)
- [Who should not use it](#who-should-not-use-it)
- [Status](#status)
- [TL;DR Quickstart](#tldr-quickstart)
- [Installation](#installation)
- [The mental model](#the-mental-model)
- [Core concepts](#core-concepts)
- [Typical workflows](#typical-workflows)
  - [1. Start a single node](#1-start-a-single-node)
  - [2. Query the live registry](#2-query-the-live-registry)
  - [3. Subscribe to lifecycle events](#3-subscribe-to-lifecycle-events)
  - [4. Update runtime state without rebuilding](#4-update-runtime-state-without-rebuilding)
- [Examples included in this repo](#examples-included-in-this-repo)
- [Public API overview](#public-api-overview)
- [Builder configuration reference](#builder-configuration-reference)
- [Runtime update semantics](#runtime-update-semantics)
- [Event model](#event-model)
- [Data advertised on the network](#data-advertised-on-the-network)
- [Failure modes and cleanup behavior](#failure-modes-and-cleanup-behavior)
- [Limitations and non-goals](#limitations-and-non-goals)
- [Testing and verification](#testing-and-verification)
- [Design notes](#design-notes)
- [Publish readiness](#publish-readiness)
- [Roadmap / likely next improvements](#roadmap--likely-next-improvements)
- [License](#license)

---

## What this crate is

`zero-conf-mesh` is a Rust library for **local-only, zero-configuration service discovery**.

It helps a set of agents, workers, tools, or small services on the same LAN:

- announce themselves automatically with mDNS/DNS-SD,
- discover peers without hardcoded IP addresses,
- maintain a live registry of visible nodes,
- react to join/update/leave events,
- evict stale peers when they disappear without shutting down cleanly.

At the current MVP level, each node advertises enough information for peers to answer questions like:

- “Who is online right now?”
- “Who is on project `alpha`?”
- “Who is working on branch `feature/mesh`?”
- “Which peer is currently `busy` vs `idle`?”

---

## Why it exists

When you run multiple coding agents or local tools in parallel, the boring part is usually not the work itself — it is coordination.

Without a shared discovery layer, you end up with some combination of:

- manually passing around hostnames or ports,
- stale hardcoded peer lists,
- ad-hoc “who is active?” logic,
- no clear signal for when a peer crashed vs shut down gracefully,
- duplicated “presence” code in every tool.

`zero-conf-mesh` exists to make that coordination layer:

- **automatic** on a shared LAN,
- **typed** rather than stringly-typed everywhere,
- **async-first** for Tokio applications,
- **small and ergonomic** instead of a giant framework,
- **observable** through lifecycle events and a queryable registry.

---

## Who should use it

This crate is a good fit if you are building:

- local multi-agent developer tools,
- workstation-side orchestrators,
- LAN-only demos or labs,
- edge or homelab nodes on a single subnet,
- small “presence-aware” tools that need peer discovery but not a service mesh.

It is especially useful if you want something simpler than:

- standing up a central registry,
- requiring user-supplied peer configuration,
- building your own heartbeat/eviction logic from scratch.

---

## Who should not use it

This is **not** the right tool if you need:

- cross-subnet or WAN discovery,
- authentication or encrypted advertisements,
- reliable message delivery,
- distributed consensus,
- leader election,
- service discovery across hostile networks.

If your environment needs trust, routing, or strong guarantees, this crate should be treated as a local presence signal only — not a full coordination plane.

---

## Status

Current implementation includes:

- async builder-driven runtime,
- local mDNS/DNS-SD advertisement,
- peer discovery via background browsing,
- in-memory registry with TTL eviction,
- lifecycle events for join/update/leave,
- test-friendly custom mDNS port support,
- runtime updates for status/project/branch/extra metadata,
- startup cleanup when initialization fails part-way through.

This crate is intended for **local-network use only**.

The core advertised TXT fields are:

- `agent_id`
- `current_project`
- `current_branch`

The current implementation also keeps richer metadata such as:

- `role`
- `status`
- arbitrary extra key/value metadata you attach at build time or runtime

---

## TL;DR Quickstart

If you just want the shortest path to a working node:

```rust
use zero_conf_mesh::{AgentStatus, ZeroConfMesh};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("reviewer")
    .project("alpha")
    .branch("main")
    .port(8080)
    .build()
    .await?;

mesh.update_status(AgentStatus::Busy).await?;

for agent in mesh.agents().await {
    println!(
        "{} {} {} {:?}",
        agent.id(),
        agent.project(),
        agent.branch(),
        agent.status()
    );
}

mesh.shutdown().await?;
# Ok(())
# }
```

That gets you:

- one local node advertising itself,
- background peer discovery,
- a live registry snapshot,
- clean shutdown.

---

## Installation

```toml
[dependencies]
zero-conf-mesh = { path = "." }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

While the crate is still local to this repo, use a path dependency.
When published, replace the path dependency with the crate version.

---

## The mental model

The easiest way to reason about `zero-conf-mesh` is:

1. **Build a config**
2. **Start one local runtime**
3. **Advertise one local announcement**
4. **Continuously browse the LAN for peers**
5. **Keep a registry of who is currently known**
6. **Emit events when peers join, change, or disappear**

You do **not** manually manage:

- IP lists,
- explicit peer registration,
- stale peer cleanup,
- background browse loops,
- heartbeat re-announcements.

The crate does that for you.

---

## Core concepts

### `ZeroConfMesh`

The main runtime handle.

This is what you build, query, update, subscribe to, and shut down.

### `ZeroConfMeshBuilder`

The typed builder used to configure:

- identity,
- role,
- project,
- branch,
- service port,
- mDNS port,
- service type,
- heartbeat interval,
- TTL,
- initial metadata.

### `AgentAnnouncement`

The wire-adjacent, serializable data that represents what a node advertises on the network.

It is used to:

- produce TXT properties,
- construct `mdns-sd` service info,
- parse remote services,
- snapshot the currently advertised local state.

### `AgentInfo`

The in-memory registry representation of a discovered node.

This is what you query from:

- `agents()`
- `get_agent(...)`
- `agents_by_project(...)`
- `agents_by_branch(...)`
- `agents_by_status(...)`

### `Registry`

The concurrent in-memory store of currently visible agents.

Most callers can ignore it and use `ZeroConfMesh` query methods directly,
but it remains available if you want advanced read access.

### Lifecycle events

The runtime emits:

- `Joined`
- `Updated`
- `Left`

with:

- `EventOrigin::{Local, Remote}`
- `DepartureReason::{Graceful, Expired}`

### TTL eviction

If a remote peer disappears without a graceful unregister, it is eventually removed when:

```text
now - last_seen > ttl
```

This makes crash recovery a first-class behavior instead of an afterthought.

---

## Typical workflows

## 1. Start a single node

```rust
use zero_conf_mesh::ZeroConfMesh;

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("worker")
    .project("demo")
    .branch("main")
    .port(7000)
    .build()
    .await?;

let local = mesh.local_agent().await;
println!("local id: {}", local.agent_id());
println!("instance: {}", local.instance_name());

mesh.shutdown().await?;
# Ok(())
# }
```

Use this when you just need one node to come online and advertise itself.

## 2. Query the live registry

```rust
use zero_conf_mesh::{AgentStatus, ZeroConfMesh};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .port(8080)
    .build()
    .await?;

let all = mesh.agents().await;
let alpha = mesh.agents_by_project("alpha").await;
let main = mesh.agents_by_branch("main").await;
let busy = mesh.agents_by_status(AgentStatus::Busy).await;

println!("all: {}", all.len());
println!("alpha: {}", alpha.len());
println!("main: {}", main.len());
println!("busy: {}", busy.len());

mesh.shutdown().await?;
# Ok(())
# }
```

This is the “who is currently on the LAN?” path.

## 3. Subscribe to lifecycle events

```rust
use zero_conf_mesh::{AgentEvent, ZeroConfMesh};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .project("alpha")
    .branch("main")
    .port(8080)
    .build()
    .await?;

let mut events = mesh.subscribe();

tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        match event {
            AgentEvent::Joined { agent, origin } => {
                println!("joined: {} ({origin:?})", agent.id());
            }
            AgentEvent::Updated { previous, current, origin } => {
                println!(
                    "updated: {} {:?} -> {:?} ({origin:?})",
                    current.id(),
                    previous.status(),
                    current.status()
                );
            }
            AgentEvent::Left { agent, origin, reason } => {
                println!("left: {} ({origin:?}, {reason:?})", agent.id());
            }
        }
    }
});

mesh.shutdown().await?;
# Ok(())
# }
```

Use this if your application reacts to membership changes in real time instead of polling.

## 4. Update runtime state without rebuilding

```rust
use zero_conf_mesh::{AgentStatus, ZeroConfMesh};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("coder")
    .project("alpha")
    .branch("main")
    .port(8080)
    .build()
    .await?;

mesh.update_status(AgentStatus::Busy).await?;
mesh.update_project("beta").await?;
mesh.update_branch("feature/runtime").await?;
mesh.update_metadata("capability", "planning").await?;

mesh.shutdown().await?;
# Ok(())
# }
```

This is useful when:

- an agent moves to a different workstream,
- status changes over time,
- you want to attach extra capabilities or labels at runtime.

---

## Examples included in this repo

### `cargo run --example single_node`

Starts one node, prints local identity and current registry contents, then shuts down cleanly.

### `cargo run --example two_nodes`

Starts two nodes in one process, waits for mutual discovery, prints lifecycle events, updates one node’s status, then shuts both down cleanly.

If your machine already has Bonjour/Avahi activity on the standard mDNS port, run the examples with a custom test port:

```bash
ZCM_MDNS_PORT=5454 cargo run --example single_node
ZCM_MDNS_PORT=5454 cargo run --example two_nodes
```

---

## Public API overview

Main entry points:

- `ZeroConfMesh::builder()`
- `ZeroConfMesh::from_config(...).await`

Common runtime methods:

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
- `update_project(...).await`
- `update_branch(...).await`
- `update_metadata(...).await`
- `shutdown().await`

In most applications, you can ignore the lower-level internals and just work through `ZeroConfMesh`.

---

## Builder configuration reference

Available builder setters:

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

Defaults:

- random UUID agent id if omitted
- role = `agent`
- project = `default`
- branch = `unknown`
- service type = `_agent-mesh._tcp.local.`
- mDNS port = `5353`
- heartbeat = `30s`
- TTL = `120s`
- initial status = `Idle`

### Notes on defaults

These are intentionally defaults, not hardcoded lock-ins:

- they are exposed as named constants,
- they are validated,
- they can be overridden via the builder.

That makes them protocol defaults / ergonomic defaults rather than “dangerous magic numbers”.

---

## Runtime update semantics

After startup, the local node can refresh selected advertised fields without rebuilding the mesh:

- `update_status(...)`
- `update_project(...)`
- `update_branch(...)`
- `update_metadata(...)`

`update_metadata(...)` is for **non-canonical extension keys** such as:

- `capability`
- `team`
- `purpose`

Canonical fields like:

- `agent_id`
- `role`
- `current_project`
- `current_branch`
- `status`

are intentionally managed through dedicated fields and updater methods so callers do not accidentally create divergent runtime state.

---

## Event model

The runtime emits three event kinds:

```rust
pub enum AgentEvent {
    Joined { agent, origin },
    Updated { previous, current, origin },
    Left { agent, origin, reason },
}
```

### Origins

- `EventOrigin::Local`
  - emitted when the local node changes its own registry state
- `EventOrigin::Remote`
  - emitted when a discovered remote peer changes state

### Departure reasons

- `DepartureReason::Graceful`
  - a peer was explicitly removed or unregistered
- `DepartureReason::Expired`
  - a peer exceeded TTL and was evicted as stale

### Practical meaning

This lets your application distinguish:

- “peer shut down intentionally”
- “peer probably crashed or vanished”
- “this update came from me”
- “this update came from the network”

---

## Data advertised on the network

The canonical TXT keys are:

- `agent_id`
- `role`
- `current_project`
- `current_branch`
- `status`

The MVP requires the first-class discovery identity to include:

- `agent_id`
- `current_project`
- `current_branch`

Extra metadata is allowed as additional string key/value pairs.

### Recommended metadata style

Keep extra TXT payloads:

- compact,
- predictable,
- UTF-8,
- small enough to stay comfortably under a few hundred bytes total.

This crate is optimized for “small presence metadata”, not arbitrary structured documents over mDNS TXT records.

---

## Failure modes and cleanup behavior

### Graceful shutdown

On `shutdown().await`, the runtime:

- signals background tasks to stop,
- unregisters the local service,
- removes the local agent from the registry,
- joins spawned tasks,
- shuts down the daemon.

### Crash / abrupt exit

No goodbye is guaranteed.

In that case, peers rely on TTL eviction:

- repeated identical announcements refresh `last_seen`,
- absent peers eventually expire,
- stale peers emit `Left { reason: Expired }`.

### Malformed remote peers

If a discovered remote service is missing required TXT properties or contains invalid TXT data:

- it is ignored,
- it is **not** partially inserted into the registry.

### Startup failure cleanup

If runtime startup fails after local announcement but before listener startup completes successfully:

- the crate cleans up the partial registration,
- the service does not remain discoverable on the network.

---

## Limitations and non-goals

This crate is intentionally **not** trying to be:

- a secure service mesh,
- a distributed systems framework,
- a cross-network service registry,
- an RPC system,
- a messaging bus.

Current limitations:

- LAN only; no cross-subnet guarantees
- no authentication or encryption of advertisements
- no reliable delivery semantics
- no leader election or consensus
- no explicit network interface control beyond what `mdns-sd` provides

---

## Testing and verification

Current automated coverage includes:

- config and builder validation,
- TXT conversion/parsing,
- registry insert/update/refresh semantics,
- removal by instance name,
- TTL eviction,
- event origin/reason semantics,
- two-node discovery on a custom mDNS port,
- remote status propagation after local updates,
- remote project/branch/metadata propagation after local updates,
- multi-peer discovery on one custom mDNS port,
- project isolation across shared-LAN discovery,
- malformed remote TXT payloads being ignored,
- startup failure cleanup for partially initialized local registration.

Run checks locally:

```bash
cargo fmt --check
cargo test --all-targets --all-features
cargo test --doc
cargo clippy --all-targets --all-features -- -D warnings
cargo package --locked
```

The CI workflow in this repo runs the same checks on pushes and pull requests.

---

## Design notes

- `ZeroConfMesh` is the main public entry point.
- `Registry` remains available for advanced read access.
- broadcaster/listener pieces are internal implementation details.
- `AgentInfo` is runtime-only and intentionally not serialized because it contains `Instant`.
- local advertisement is stored behind `Arc<RwLock<...>>` so runtime updates remain simple and explicit.
- shutdown and background-task coordination use Tokio primitives rather than custom concurrency machinery.

The design goal is not cleverness — it is correctness, observability, and a small ergonomic API.

---

## Publish readiness

Current crate state is suitable for continued public packaging work:

- library docs are enforced with `missing_docs`,
- `unsafe` is forbidden,
- tests, doctests, clippy, and package verification run clean,
- examples are included for quick validation,
- package contents are explicitly constrained,
- spec and README are aligned with the current implementation.

Before publishing to crates.io, you would still typically want to:

- add richer crate-level rustdoc examples over time,
- add crates.io keywords/categories polish,
- optionally expand CI matrix coverage if your target environments justify it.

---

## Roadmap / likely next improvements

Things that would be natural to add next, depending on real-world usage:

- configurable event channel capacity in the public runtime builder,
- richer status vocabularies or user-defined states,
- additional query helpers for roles/capabilities,
- explicit metadata removal / replacement helpers,
- more explicit interface-selection controls if deployments require them.

Importantly, those are **potential improvements**, not prerequisites for the current MVP.

---

## License

MIT
