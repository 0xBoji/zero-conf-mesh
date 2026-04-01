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

impl AgentEvent {
    /// Returns the primary agent snapshot associated with this event.
    #[must_use]
    pub fn agent(&self) -> &AgentInfo {
        match self {
            Self::Joined { agent, .. } | Self::Left { agent, .. } => agent,
            Self::Updated { current, .. } => current,
        }
    }

    /// Returns the event origin.
    #[must_use]
    pub const fn origin(&self) -> EventOrigin {
        match self {
            Self::Joined { origin, .. }
            | Self::Updated { origin, .. }
            | Self::Left { origin, .. } => *origin,
        }
    }

    /// Returns the previous snapshot for update events.
    #[must_use]
    pub const fn previous(&self) -> Option<&AgentInfo> {
        match self {
            Self::Updated { previous, .. } => Some(previous),
            Self::Joined { .. } | Self::Left { .. } => None,
        }
    }

    /// Returns the departure reason for leave events.
    #[must_use]
    pub const fn departure_reason(&self) -> Option<DepartureReason> {
        match self {
            Self::Left { reason, .. } => Some(*reason),
            Self::Joined { .. } | Self::Updated { .. } => None,
        }
    }

    /// Returns true when this event describes a join.
    #[must_use]
    pub const fn is_joined(&self) -> bool {
        matches!(self, Self::Joined { .. })
    }

    /// Returns true when this event describes an update.
    #[must_use]
    pub const fn is_updated(&self) -> bool {
        matches!(self, Self::Updated { .. })
    }

    /// Returns true when this event describes a leave.
    #[must_use]
    pub const fn is_left(&self) -> bool {
        matches!(self, Self::Left { .. })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr},
        time::Instant,
    };

    use super::*;
    use crate::types::{AgentAnnouncement, AgentMetadata, AgentStatus};

    fn agent(id: &str) -> AgentInfo {
        AgentAnnouncement::new(
            format!("{id}._agent-mesh._tcp.local."),
            id,
            "coder",
            "alpha",
            "main",
            AgentStatus::Idle,
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            AgentMetadata::new(),
        )
        .expect("announcement should be valid")
        .into_agent_info(Instant::now())
    }

    #[test]
    fn agent_event_helpers_should_expose_common_fields() {
        let previous = agent("agent-a");
        let current = agent("agent-b");
        let event = AgentEvent::Updated {
            previous,
            current,
            origin: EventOrigin::Remote,
        };

        assert_eq!(event.agent().id(), "agent-b");
        assert_eq!(event.origin(), EventOrigin::Remote);
        assert_eq!(event.previous().map(|agent| agent.id()), Some("agent-a"));
        assert_eq!(event.departure_reason(), None);
        assert!(event.is_updated());
        assert!(!event.is_joined());
        assert!(!event.is_left());
    }
}
