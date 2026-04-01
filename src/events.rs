use crate::types::AgentInfo;

/// Registry event emitted when an agent enters, changes, or leaves the mesh.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AgentEvent {
    /// A new agent was added to the registry.
    Joined(AgentInfo),
    /// An existing agent changed one or more advertised properties.
    Updated {
        /// The previous registry state.
        previous: AgentInfo,
        /// The new registry state.
        current: AgentInfo,
    },
    /// An agent left gracefully.
    Left(AgentInfo),
    /// An agent was removed because it exceeded the configured TTL.
    Expired(AgentInfo),
}
