#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

//! `coding_agent_mesh_presence` provides the building blocks for zero-configuration agent
//! discovery on a local network.
//!
//! The crate is being built incrementally. The current slice provides:
//! - strongly typed configuration and domain models,
//! - an async-safe in-memory registry,
//! - TTL-based eviction,
//! - mDNS/DNS-SD broadcasting for the local node,
//! - background browsing for peer discovery,
//! - a builder-driven [`ZeroConfMesh`] runtime.
//!
//! # Example
//! ```no_run
//! use coding_agent_mesh_presence::{AgentStatus, ZeroConfMesh};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mesh = ZeroConfMesh::builder()
//!     .agent_id("agent-01")
//!     .role("reviewer")
//!     .project("alpha")
//!     .branch("main")
//!     .port(8080)
//!     .build()
//!     .await?;
//!
//! mesh.update_status(AgentStatus::Busy).await?;
//! let peers = mesh.agents_by_project("alpha").await;
//! println!("known peers: {}", peers.len());
//! mesh.shutdown().await?;
//! # Ok(())
//! # }
//! ```

mod broadcaster;
mod builder;
mod config;
mod error;
mod events;
mod listener;
mod mesh;
mod registry;
mod types;

pub use builder::ZeroConfMeshBuilder;
pub use config::{
    DEFAULT_EVENT_CAPACITY, DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_MDNS_PORT, DEFAULT_SERVICE_TYPE,
    DEFAULT_TTL, NetworkInterface, SharedSecretAuth, SharedSecretMode, ZeroConfConfig,
};
pub use error::ZeroConfError;
pub use events::{AgentEvent, DepartureReason, EventOrigin};
pub use mesh::ZeroConfMesh;
pub use registry::{Registry, RegistryUpsert};
pub use types::{
    AGENT_AUTH_SCHEME_METADATA_KEY, AGENT_BRANCH_METADATA_KEY, AGENT_CAPABILITIES_METADATA_KEY,
    AGENT_ID_METADATA_KEY, AGENT_PROJECT_METADATA_KEY, AGENT_ROLE_METADATA_KEY,
    AGENT_SIGNATURE_METADATA_KEY, AGENT_STATUS_METADATA_KEY, AgentAnnouncement, AgentInfo,
    AgentMetadata, AgentStatus,
};
