use std::{net::IpAddr, time::Duration};

use crate::{
    error::ZeroConfError,
    types::{
        AGENT_BRANCH_METADATA_KEY, AGENT_ID_METADATA_KEY, AGENT_PROJECT_METADATA_KEY,
        AGENT_ROLE_METADATA_KEY, AGENT_STATUS_METADATA_KEY, AgentAnnouncement, AgentMetadata,
        AgentStatus, is_reserved_metadata_key,
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

/// Determines whether a configured shared secret only signs local announcements
/// or also verifies incoming peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharedSecretMode {
    /// Sign local announcements, but do not reject unsigned or invalid remote peers.
    SignOnly,
    /// Sign local announcements and require valid signatures from remote peers.
    SignAndVerify,
}

/// Optional shared-secret authentication settings for LAN announcements.
#[derive(Clone, PartialEq, Eq)]
pub struct SharedSecretAuth {
    signing_secret: String,
    verification_secrets: Vec<String>,
    mode: SharedSecretMode,
}

impl std::fmt::Debug for SharedSecretAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedSecretAuth")
            .field("signing_secret", &"<redacted>")
            .field(
                "verification_secret_count",
                &self.verification_secrets.len(),
            )
            .field("mode", &self.mode)
            .finish()
    }
}

impl SharedSecretAuth {
    /// Creates validated shared-secret authentication settings.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the secret is empty after trimming.
    pub fn new(secret: impl Into<String>, mode: SharedSecretMode) -> Result<Self, ZeroConfError> {
        let signing_secret = normalize_shared_secret(secret.into())?;
        Ok(Self {
            verification_secrets: vec![signing_secret.clone()],
            signing_secret,
            mode,
        })
    }

    /// Creates shared-secret authentication with explicit rotation support.
    ///
    /// The first secret is used for signing local announcements; all provided
    /// secrets, plus the signing secret, are accepted for remote verification.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when any secret is empty after trimming.
    pub fn with_rotation<I, S>(
        signing_secret: impl Into<String>,
        verification_secrets: I,
        mode: SharedSecretMode,
    ) -> Result<Self, ZeroConfError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let signing_secret = normalize_shared_secret(signing_secret.into())?;
        let mut normalized = verification_secrets
            .into_iter()
            .map(|secret| normalize_shared_secret(secret.into()))
            .collect::<Result<Vec<_>, _>>()?;

        if !normalized.iter().any(|secret| secret == &signing_secret) {
            normalized.push(signing_secret.clone());
        }

        normalized.sort();
        normalized.dedup();

        Ok(Self {
            signing_secret,
            verification_secrets: normalized,
            mode,
        })
    }

    /// Returns the configured verification mode.
    #[must_use]
    pub const fn mode(&self) -> SharedSecretMode {
        self.mode
    }

    /// Returns whether remote peers should be verified.
    #[must_use]
    pub const fn verifies_incoming(&self) -> bool {
        matches!(self.mode, SharedSecretMode::SignAndVerify)
    }

    pub(crate) fn signing_secret(&self) -> &str {
        &self.signing_secret
    }

    /// Returns the accepted verification secrets for incoming peers.
    #[must_use]
    pub fn verification_secrets(&self) -> &[String] {
        &self.verification_secrets
    }
}

/// Selects which network interfaces the embedded mDNS daemon should include or exclude.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NetworkInterface {
    /// Match all interfaces.
    All,
    /// Match all IPv4 interfaces.
    IPv4,
    /// Match all IPv6 interfaces.
    IPv6,
    /// Match a specific interface by system name, such as `en0`.
    Name(String),
    /// Match an interface by one of its assigned IP addresses.
    Addr(IpAddr),
    /// Match the IPv4 loopback interface.
    LoopbackV4,
    /// Match the IPv6 loopback interface.
    LoopbackV6,
    /// Match an IPv4 interface by index.
    IndexV4(u32),
    /// Match an IPv6 interface by index.
    IndexV6(u32),
}

impl From<&str> for NetworkInterface {
    fn from(value: &str) -> Self {
        Self::Name(value.to_owned())
    }
}

impl From<String> for NetworkInterface {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}

impl From<IpAddr> for NetworkInterface {
    fn from(value: IpAddr) -> Self {
        Self::Addr(value)
    }
}

impl NetworkInterface {
    pub(crate) fn to_mdns_if_kind(&self) -> mdns_sd::IfKind {
        match self {
            Self::All => mdns_sd::IfKind::All,
            Self::IPv4 => mdns_sd::IfKind::IPv4,
            Self::IPv6 => mdns_sd::IfKind::IPv6,
            Self::Name(name) => mdns_sd::IfKind::Name(name.clone()),
            Self::Addr(addr) => mdns_sd::IfKind::Addr(*addr),
            Self::LoopbackV4 => mdns_sd::IfKind::LoopbackV4,
            Self::LoopbackV6 => mdns_sd::IfKind::LoopbackV6,
            Self::IndexV4(index) => mdns_sd::IfKind::IndexV4(*index),
            Self::IndexV6(index) => mdns_sd::IfKind::IndexV6(*index),
        }
    }
}

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
    capabilities: Vec<String>,
    metadata: AgentMetadata,
    advertise_local: bool,
    enabled_interfaces: Vec<NetworkInterface>,
    disabled_interfaces: Vec<NetworkInterface>,
    shared_secret_auth: Option<SharedSecretAuth>,
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
        capabilities: Vec<String>,
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
            capabilities,
            metadata,
            advertise_local: true,
            enabled_interfaces: Vec::new(),
            disabled_interfaces: Vec::new(),
            shared_secret_auth: None,
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

        if let Some(key) = self
            .metadata
            .keys()
            .find(|key| is_reserved_metadata_key(key))
            .cloned()
        {
            return Err(ZeroConfError::ReservedMetadataKey { key });
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

    /// Returns the typed capabilities configured for the local node.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// Returns any extra metadata configured for the local node.
    #[must_use]
    pub const fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// Returns whether this node should announce itself on the LAN.
    #[must_use]
    pub const fn advertise_local(&self) -> bool {
        self.advertise_local
    }

    /// Returns the shared-secret authentication settings, if enabled.
    #[must_use]
    pub fn shared_secret_auth(&self) -> Option<&SharedSecretAuth> {
        self.shared_secret_auth.as_ref()
    }

    /// Returns the interfaces explicitly enabled for the embedded mDNS daemon.
    #[must_use]
    pub fn enabled_interfaces(&self) -> &[NetworkInterface] {
        &self.enabled_interfaces
    }

    /// Returns the interfaces explicitly excluded for the embedded mDNS daemon.
    #[must_use]
    pub fn disabled_interfaces(&self) -> &[NetworkInterface] {
        &self.disabled_interfaces
    }

    /// Adds an interface inclusion rule to the configuration.
    #[must_use]
    pub fn with_enabled_interface(mut self, interface: impl Into<NetworkInterface>) -> Self {
        self.enabled_interfaces.push(interface.into());
        self
    }

    /// Controls whether the local node should announce itself on the LAN.
    #[must_use]
    pub fn with_advertise_local(mut self, advertise_local: bool) -> Self {
        self.advertise_local = advertise_local;
        self
    }

    /// Adds multiple interface inclusion rules to the configuration.
    #[must_use]
    pub fn with_enabled_interfaces<I, T>(mut self, interfaces: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<NetworkInterface>,
    {
        self.enabled_interfaces
            .extend(interfaces.into_iter().map(Into::into));
        self
    }

    /// Adds an interface exclusion rule to the configuration.
    #[must_use]
    pub fn with_disabled_interface(mut self, interface: impl Into<NetworkInterface>) -> Self {
        self.disabled_interfaces.push(interface.into());
        self
    }

    /// Adds multiple interface exclusion rules to the configuration.
    #[must_use]
    pub fn with_disabled_interfaces<I, T>(mut self, interfaces: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<NetworkInterface>,
    {
        self.disabled_interfaces
            .extend(interfaces.into_iter().map(Into::into));
        self
    }

    /// Enables shared-secret authentication for the local node.
    #[must_use]
    pub fn with_shared_secret_auth(mut self, auth: SharedSecretAuth) -> Self {
        self.shared_secret_auth = Some(auth);
        self
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
        crate::types::sync_capabilities_metadata(&mut metadata, &self.capabilities);

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

fn normalize_shared_secret(secret: String) -> Result<String, ZeroConfError> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err(ZeroConfError::EmptySharedSecret);
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
            AgentMetadata::new(),
        )
        .expect_err("event capacity zero must be rejected");

        assert_eq!(err.to_string(), "event capacity must be greater than zero");
    }
}
