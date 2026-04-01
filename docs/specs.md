# Technical Specification: `zero-conf-mesh`

## 1. Overview
`zero-conf-mesh` is a Rust crate providing a decentralized Service Discovery mechanism for multi-agent systems within a Local Area Network (LAN). Built on top of mDNS (Multicast DNS) and DNS-SD, this crate enables autonomous nodes to dynamically discover, connect, and share states without relying on any central server or database.

## 2. Motivation
In distributed systems, particularly when building toolkits for autonomous LLM agents (e.g., agents coordinating automated trading strategies, data scraping, or code reviewing), hardcoding IPs or setting up a Service Registry (like Consul or ZooKeeper) is overkill and lacks flexibility for local/edge environments.

This project solves a specific problem: **How do we drop N agents into the same local network and have them autonomously figure out "Who is where, and what are they doing?"**

## 3. Architecture
The system consists of three main concurrent components:

1. **Broadcaster:** Continuously announces the current agent's presence via mDNS broadcast.
2. **Listener:** Listens to mDNS traffic to discover other agents sharing the `_agent-mesh._tcp` service type.
3. **Registry:** A thread-safe, in-memory state store that maintains and updates the metadata of all discovered agents in real-time.

### Data Flow
1. `Agent A` starts -> Updates its Local Registry -> Broadcaster announces presence via mDNS with TXT records.
2. `Agent B` (running Listener) -> Captures `Agent A`'s packet -> Parses TXT records -> Saves `Agent A` into `Agent B`'s Local Registry.
3. When `Agent A` shuts down gracefully -> Sends a "Goodbye" mDNS packet -> `Agent B` removes `Agent A` from its Registry.

## 4. Data Models

### 4.1. Network Identifier
- **Service Type:** `_agent-mesh._tcp.local.`
- **Instance Name:** A unique name generated randomly or based on the Agent ID (e.g., `agent-01._agent-mesh._tcp.local.`).

### 4.2. Metadata (TXT Records)
mDNS TXT record payloads should ideally be kept under 400 bytes. Required structure:
- `agent_id` (String): Unique identifier (UUID/Snowflake).
- `role` (String): The agent's role (e.g., `coder`, `reviewer`, `project-manager`).
- `current_project` (String): Project space for grouping/isolation.
- `status` (String): Current operational status (`idle`, `busy`, `error`).

### 4.3. Registry State (`AgentInfo`)
```rust
struct AgentInfo {
    id: String,
    ip_addresses: Vec<IpAddr>,
    port: u16,
    metadata: HashMap<String, String>,
    last_seen: Instant,
}
```

## 5. Public API Surface (Proposed)
The Developer API should be simple and declarative.

```rust
// Initialize the Mesh with configuration
let mut mesh = ZeroConfMesh::builder()
    .agent_id("agent-01")
    .role("project-manager")
    .project("iOS-liquid-glass")
    .port(8080)
    .build()
    .await?;

// Retrieve active agents within a specific project
let active_agents = mesh.registry().get_all_by_project("iOS-liquid-glass");

// Update status (triggers a new TXT record broadcast)
mesh.update_status("busy").await?;
```

## 6. Edge Cases & Constraints
- **Network Isolation:** mDNS does not route across Subnets/VLANs (operates at Layer 2 Broadcast). This is strictly for local network use.
- **Multiple Network Interfaces:** Machines may have multiple NICs (Wi-Fi, Docker bridge, VPN). The system must bind to the correct interface or broadcast across all available interfaces (`0.0.0.0`).
- **Split Brain / Zombie Nodes:** If an agent crashes unexpectedly (fails to send a "Goodbye" packet), the Registry must implement a TTL (Time-To-Live) eviction policy to purge stale nodes that haven't sent a heartbeat within a specific timeframe (e.g., 120 seconds).

## 7. Future Roadmap
- Support for encrypted TXT record payloads (Security).
- Integration of automatic Leader Election among nodes.
```

Where the specification is incomplete or would benefit from engineering depth, expand it with production-quality detail. Specifically:

- **Section 3 (Architecture):** Add a concurrency model explanation — how the Broadcaster, Listener, and Registry interact using Rust async primitives (`tokio`, `Arc<RwLock<>>`, channels, or tasks). Include a component diagram in ASCII or mermaid if it aids clarity.
- **Section 4 (Data Models):** Flesh out the `AgentInfo` struct with derived traits, visibility modifiers, and any supporting types (e.g., `AgentStatus` enum). Add serialization considerations.
- **Section 5 (API Surface):** Expand with error types (`ZeroConfError`), the full builder pattern struct, and any event/callback hooks for agent join/leave events.
- **Section 6 (Edge Cases):** Add implementation guidance for the TTL eviction mechanism — specifically how a background `tokio::task` should sweep the registry and the recommended heartbeat interval vs. TTL ratio.
- **Add Section 8 — Crate Dependencies:** List the recommended Rust crates (e.g., `mdns-sd`, `tokio`, `uuid`, `serde`) with justification for each.
- **Add Section 9 — Testing Strategy:** Cover unit tests for the Registry, integration tests using loopback mDNS, and how to simulate agent crash/TTL eviction in tests.

Write the output as a single, complete, ready-to-commit `specs.md` file in valid Markdown. Do not include any commentary outside the document itself.