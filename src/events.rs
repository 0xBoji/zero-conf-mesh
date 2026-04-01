use crate::types::AgentInfo;

/// Indicates whether a registry event originated from the local node or a remote peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventOrigin {
    /// The event was produced by the local node.
    Local,
    /// The event was produced by a discovered remote peer.
    Remote,
}

/// Explains why an agent left the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepartureReason {
    /// The agent was removed explicitly, e.g. via a graceful goodbye/unregister flow.
    Graceful,
    /// The agent was evicted after exceeding the configured TTL.
    Expired,
}

/// Registry event emitted when an agent enters, changes, or leaves the mesh.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AgentEvent {
    /// A new agent was added to the registry.
    Joined {
        /// The agent snapshot that joined.
        agent: AgentInfo,
        /// The origin of the event.
        origin: EventOrigin,
    },
    /// An existing agent changed one or more advertised properties.
    Updated {
        /// The previous registry state.
        previous: AgentInfo,
        /// The new registry state.
        current: AgentInfo,
        /// The origin of the event.
        origin: EventOrigin,
    },
    /// An agent left the registry.
    Left {
        /// The agent snapshot at removal time.
        agent: AgentInfo,
        /// The origin of the event.
        origin: EventOrigin,
        /// The reason for removal.
        reason: DepartureReason,
    },
}
