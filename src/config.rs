use std::time::Duration;

use crate::{
    error::ZeroConfError,
    types::{
        AGENT_BRANCH_METADATA_KEY, AGENT_ID_METADATA_KEY, AGENT_PROJECT_METADATA_KEY,
        AGENT_ROLE_METADATA_KEY, AGENT_STATUS_METADATA_KEY, AgentAnnouncement, AgentMetadata,
        AgentStatus,
    },
};

/// Default DNS-SD service type advertised by the crate.
pub const DEFAULT_SERVICE_TYPE: &str = "_agent-mesh._tcp.local.";
/// Default UDP port used by the embedded mDNS daemon.
pub const DEFAULT_MDNS_PORT: u16 = mdns_sd::MDNS_PORT;
/// Default interval between local refresh heartbeats.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Default TTL used for stale-peer eviction.
pub const DEFAULT_TTL: Duration = Duration::from_secs(120);
/// Default broadcast channel capacity for lifecycle events.
pub const DEFAULT_EVENT_CAPACITY: usize = 256;

/// Immutable runtime configuration for a mesh node.
///
/// This is usually constructed via [`crate::ZeroConfMeshBuilder`], but can also
/// be created directly for advanced embedding and testing scenarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroConfConfig {
    agent_id: String,
    role: String,
    project: String,
    branch: String,
    port: u16,
    mdns_port: u16,
    service_type: String,
    initial_status: AgentStatus,
    heartbeat_interval: Duration,
    ttl: Duration,
    event_capacity: usize,
    metadata: AgentMetadata,
}

impl ZeroConfConfig {
    /// Creates a new validated configuration.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the provided fields are invalid.
    #[expect(
        clippy::too_many_arguments,
        reason = "configuration is assembled explicitly before networking layers exist"
    )]
    pub fn new(
        agent_id: impl Into<String>,
        role: impl Into<String>,
        project: impl Into<String>,
        branch: impl Into<String>,
        port: u16,
        mdns_port: u16,
        service_type: impl Into<String>,
        initial_status: AgentStatus,
        heartbeat_interval: Duration,
        ttl: Duration,
        event_capacity: usize,
        metadata: AgentMetadata,
    ) -> Result<Self, ZeroConfError> {
        let config = Self {
            agent_id: normalize_required(agent_id.into(), "agent_id")?,
            role: normalize_required(role.into(), "role")?,
            project: normalize_required(project.into(), "project")?,
            branch: normalize_required(branch.into(), "branch")?,
            port,
            mdns_port,
            service_type: normalize_required(service_type.into(), "service_type")?,
            initial_status,
            heartbeat_interval,
            ttl,
            event_capacity,
            metadata,
        };

        config.validate()?;
        Ok(config)
    }

    /// Validates semantic constraints for the configuration.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the configuration is invalid.
    pub fn validate(&self) -> Result<(), ZeroConfError> {
        if self.port == 0 {
            return Err(ZeroConfError::InvalidPort);
        }

        if self.mdns_port == 0 {
            return Err(ZeroConfError::InvalidMdnsPort);
        }

        if self.ttl <= self.heartbeat_interval {
            return Err(ZeroConfError::InvalidTiming {
                heartbeat_interval: self.heartbeat_interval,
                ttl: self.ttl,
            });
        }

        if self.event_capacity == 0 {
            return Err(ZeroConfError::InvalidEventCapacity);
        }

        if !is_valid_service_type(&self.service_type) {
            return Err(ZeroConfError::InvalidServiceType {
                service_type: self.service_type.clone(),
            });
        }

        if self.metadata.keys().any(|key| key.trim().is_empty()) {
            return Err(ZeroConfError::EmptyMetadataKey);
        }

        Ok(())
    }

    /// Returns the unique identifier for the local agent.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Returns the configured role.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the configured project namespace.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Returns the configured branch or workstream identifier.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the advertised listening port.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Returns the UDP port used by the internal mDNS daemon.
    #[must_use]
    pub const fn mdns_port(&self) -> u16 {
        self.mdns_port
    }

    /// Returns the DNS-SD service type.
    #[must_use]
    pub fn service_type(&self) -> &str {
        &self.service_type
    }

    /// Returns the initial local status.
    #[must_use]
    pub const fn initial_status(&self) -> AgentStatus {
        self.initial_status
    }

    /// Returns the local heartbeat interval.
    #[must_use]
    pub const fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    /// Returns the TTL applied to remote agents.
    #[must_use]
    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Returns the lifecycle event broadcast channel capacity.
    #[must_use]
    pub const fn event_capacity(&self) -> usize {
        self.event_capacity
    }

    /// Returns any extra metadata configured for the local node.
    #[must_use]
    pub const fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// Returns the computed DNS-SD instance name for this agent.
    #[must_use]
    pub fn instance_name(&self) -> String {
        format!("{}.{}", self.agent_id, self.service_type)
    }

    /// Returns the host name used for the local mDNS service record.
    #[must_use]
    pub fn host_name(&self) -> String {
        format!("{}.local.", self.agent_id)
    }

    pub(crate) fn local_announcement(&self) -> Result<AgentAnnouncement, ZeroConfError> {
        let mut metadata = self.metadata.clone();
        metadata.insert(AGENT_ID_METADATA_KEY.to_owned(), self.agent_id.clone());
        metadata.insert(AGENT_ROLE_METADATA_KEY.to_owned(), self.role.clone());
        metadata.insert(AGENT_PROJECT_METADATA_KEY.to_owned(), self.project.clone());
        metadata.insert(AGENT_BRANCH_METADATA_KEY.to_owned(), self.branch.clone());
        metadata.insert(
            AGENT_STATUS_METADATA_KEY.to_owned(),
            self.initial_status.as_str().to_owned(),
        );

        AgentAnnouncement::new(
            self.instance_name(),
            self.agent_id.clone(),
            self.role.clone(),
            self.project.clone(),
            self.branch.clone(),
            self.initial_status,
            self.port,
            Vec::new(),
            metadata,
        )
    }
}

fn normalize_required(value: String, field: &'static str) -> Result<String, ZeroConfError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ZeroConfError::EmptyField { field });
    }
    Ok(trimmed.to_owned())
}

fn is_valid_service_type(service_type: &str) -> bool {
    if !service_type.starts_with('_') || !service_type.ends_with(".local.") {
        return false;
    }

    service_type.contains("._tcp.") || service_type.contains("._udp.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_should_reject_zero_port() {
        let err = ZeroConfConfig::new(
            "agent-1",
            "coder",
            "proj",
            "main",
            0,
            DEFAULT_MDNS_PORT,
            DEFAULT_SERVICE_TYPE,
            AgentStatus::Idle,
            DEFAULT_HEARTBEAT_INTERVAL,
            DEFAULT_TTL,
            DEFAULT_EVENT_CAPACITY,
            AgentMetadata::new(),
        )
        .expect_err("port zero must be rejected");

        assert_eq!(err.to_string(), "port must be greater than zero");
    }

    #[test]
    fn config_should_reject_ttl_not_greater_than_heartbeat() {
        let err = ZeroConfConfig::new(
            "agent-1",
            "coder",
            "proj",
            "main",
            8080,
            DEFAULT_MDNS_PORT,
            DEFAULT_SERVICE_TYPE,
            AgentStatus::Idle,
            Duration::from_secs(30),
            Duration::from_secs(30),
            DEFAULT_EVENT_CAPACITY,
            AgentMetadata::new(),
        )
        .expect_err("ttl must be greater than heartbeat");

        assert!(
            err.to_string()
                .contains("ttl (30s) must be greater than heartbeat interval (30s)")
        );
    }

    #[test]
    fn config_should_reject_zero_mdns_port() {
        let err = ZeroConfConfig::new(
            "agent-1",
            "coder",
            "proj",
            "main",
            8080,
            0,
            DEFAULT_SERVICE_TYPE,
            AgentStatus::Idle,
            DEFAULT_HEARTBEAT_INTERVAL,
            DEFAULT_TTL,
            DEFAULT_EVENT_CAPACITY,
            AgentMetadata::new(),
        )
        .expect_err("mDNS port zero must be rejected");

        assert_eq!(err.to_string(), "mDNS port must be greater than zero");
    }

    #[test]
    fn config_should_reject_zero_event_capacity() {
        let err = ZeroConfConfig::new(
            "agent-1",
            "coder",
            "proj",
            "main",
            8080,
            DEFAULT_MDNS_PORT,
            DEFAULT_SERVICE_TYPE,
            AgentStatus::Idle,
            DEFAULT_HEARTBEAT_INTERVAL,
            DEFAULT_TTL,
            0,
            AgentMetadata::new(),
        )
        .expect_err("event capacity zero must be rejected");

        assert_eq!(err.to_string(), "event capacity must be greater than zero");
    }
}
