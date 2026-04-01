# zero-conf-mesh

Zero-configuration LAN service discovery for multi-agent systems in Rust using mDNS/DNS-SD.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](#installation)

## Status

Current implementation includes:
- async builder-driven runtime,
- local mDNS/DNS-SD advertisement,
- peer discovery via background browsing,
- in-memory registry with TTL eviction,
- lifecycle events for join/update/leave,
- test-friendly custom mDNS port support.

This crate is intended for **local-network use only**. It does not provide cross-subnet discovery, authentication, encryption, or consensus.

## Why this crate?

`zero-conf-mesh` is useful when you want multiple local agents or tools to:
- appear automatically on a shared LAN,
- discover peers without hardcoded IPs,
- expose lightweight runtime metadata,
- maintain a live in-process peer registry.

Typical use cases:
- local multi-agent development,
- workstation-side orchestration,
- lab/demo deployments,
- edge or homelab coordination on one subnet.

## Features

- **Minimal async API** via `ZeroConfMesh`
- **Typed metadata and status**
- **Graceful shutdown** with unregister flow
- **Crash-tolerant cleanup** via TTL sweeps
- **Lifecycle events** with:
  - `EventOrigin::{Local, Remote}`
  - `DepartureReason::{Graceful, Expired}`

## Installation

```toml
[dependencies]
zero-conf-mesh = { path = "." }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

When published, replace the path dependency with the crate version.

## Quick Start

```rust
use zero_conf_mesh::{AgentStatus, ZeroConfMesh};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("reviewer")
    .project("alpha")
    .port(8080)
    .build()
    .await?;

mesh.update_status(AgentStatus::Busy).await?;

let local = mesh.local_agent().await;
let peers = mesh.agents_by_project("alpha").await;
let maybe_peer = mesh.get_agent("agent-02").await;

println!("local agent: {}", local.agent_id());
println!("known peers: {}", peers.len());
println!("agent-02 visible: {}", maybe_peer.is_some());

mesh.shutdown().await?;
# Ok(())
# }
```

## Included Examples

- `cargo run --example single_node`
  - starts one node,
  - prints local identity and registry contents,
  - shuts down cleanly.

- `cargo run --example two_nodes`
  - starts two nodes in one process,
  - waits for mutual discovery,
  - prints lifecycle events,
  - updates one node's status,
  - shuts both down cleanly.

If the standard mDNS port conflicts with your machine's Bonjour/Avahi setup, run either example with:

```bash
ZCM_MDNS_PORT=5454 cargo run --example single_node
ZCM_MDNS_PORT=5454 cargo run --example two_nodes
```

## Event Subscription

```rust
use zero_conf_mesh::{AgentEvent, DepartureReason, EventOrigin, ZeroConfMesh};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("coder")
    .project("alpha")
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

## Builder Options

Available builder setters:
- `agent_id(...)`
- `role(...)`
- `project(...)`
- `port(...)`
- `mdns_port(...)`
- `service_type(...)`
- `status(...)`
- `heartbeat_interval(...)`
- `ttl(...)`
- `metadata(key, value)`
- `metadata_map(...)`

Important defaults:
- random UUID agent id if omitted,
- role = `agent`,
- project = `default`,
- service type = `_agent-mesh._tcp.local.`,
- mDNS port = `5353`,
- heartbeat = `30s`,
- TTL = `120s`.

## Example

Run the included demos:

```bash
cargo run --example single_node
cargo run --example two_nodes
```

## Design Notes

- `ZeroConfMesh` is the main public entry point.
- `Registry` remains available for advanced read access.
- low-level broadcaster/listener pieces are internal implementation details.
- `AgentInfo` is runtime-only and intentionally not serialized because it stores `Instant`.

## Testing

Current automated coverage includes:
- config and builder validation,
- TXT conversion/parsing,
- registry insert/update/refresh semantics,
- removal by instance name,
- TTL eviction,
- event origin/reason semantics,
- two-node discovery on a custom mDNS port.

Run checks locally:

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Publish Readiness

Current crate state is suitable for continued public packaging work:
- library docs are enforced with `missing_docs`,
- `unsafe` is forbidden,
- tests and clippy run clean,
- examples are included for quick validation,
- spec and README are aligned with the current implementation.

Before publishing to crates.io, you would still typically want to:
- replace the placeholder repository URL in `Cargo.toml`,
- add crate-level API examples in rustdoc,
- optionally add CI metadata/badges once the repo is public.

## Limitations

- LAN only; no cross-subnet guarantees
- no security layer on metadata
- no leader election or higher-level coordination primitives yet

## License

MIT
