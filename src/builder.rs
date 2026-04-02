use std::time::Duration;

use uuid::Uuid;

use crate::{
    config::{
        DEFAULT_EVENT_CAPACITY, DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_MDNS_PORT,
        DEFAULT_SERVICE_TYPE, DEFAULT_TTL, NetworkInterface, SharedSecretAuth, SharedSecretMode,
        ZeroConfConfig,
    },
    error::ZeroConfError,
    mesh::ZeroConfMesh,
    types::{AgentMetadata, AgentStatus, canonicalize_capabilities},
};

#[derive(Debug, Clone)]
struct SharedSecretBuilderConfig {
    signing_secret: String,
    verification_secrets: Vec<String>,
    mode: SharedSecretMode,
}

/// Builder for constructing a [`ZeroConfMesh`] instance.
///
/// # Example
/// ```no_run
/// use coding_agent_mesh_presence::ZeroConfMesh;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mesh = ZeroConfMesh::builder()
///     .agent_id("agent-01")
///     .role("coder")
///     .project("alpha")
///     .branch("main")
///     .port(8080)
///     .build()
///     .await?;
///
/// mesh.shutdown().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ZeroConfMeshBuilder {
    agent_id: Option<String>,
    role: String,
    project: String,
    branch: String,
    port: Option<u16>,
    mdns_port: u16,
    service_type: String,
    initial_status: AgentStatus,
    heartbeat_interval: Duration,
    ttl: Duration,
    event_capacity: usize,
    metadata: AgentMetadata,
    capabilities: Vec<String>,
    advertise_local: bool,
    enabled_interfaces: Vec<NetworkInterface>,
    disabled_interfaces: Vec<NetworkInterface>,
    shared_secret_auth: Option<SharedSecretBuilderConfig>,
}

impl Default for ZeroConfMeshBuilder {
    fn default() -> Self {
        Self {
            agent_id: None,
            role: "agent".to_owned(),
            project: "default".to_owned(),
            branch: "unknown".to_owned(),
            port: None,
            mdns_port: DEFAULT_MDNS_PORT,
            service_type: DEFAULT_SERVICE_TYPE.to_owned(),
            initial_status: AgentStatus::Idle,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            ttl: DEFAULT_TTL,
            event_capacity: DEFAULT_EVENT_CAPACITY,
            metadata: AgentMetadata::new(),
            capabilities: Vec::new(),
            advertise_local: true,
            enabled_interfaces: Vec::new(),
            disabled_interfaces: Vec::new(),
            shared_secret_auth: None,
        }
    }
}

impl ZeroConfMeshBuilder {
    /// Sets the local agent identifier.
    #[must_use]
    pub fn agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// Sets the local agent role.
    #[must_use]
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = role.into();
        self
    }

    /// Sets the project namespace used for discovery grouping.
    #[must_use]
    pub fn project(mut self, project: impl Into<String>) -> Self {
        self.project = project.into();
        self
    }

    /// Sets the current branch or workstream identifier to advertise.
    #[must_use]
    pub fn branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = branch.into();
        self
    }

    /// Sets the advertised service port.
    #[must_use]
    pub const fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Sets the UDP port used by the internal mDNS daemon.
    #[must_use]
    pub const fn mdns_port(mut self, mdns_port: u16) -> Self {
        self.mdns_port = mdns_port;
        self
    }

    /// Overrides the DNS-SD service type.
    #[must_use]
    pub fn service_type(mut self, service_type: impl Into<String>) -> Self {
        self.service_type = service_type.into();
        self
    }

    /// Sets the initial local agent status.
    #[must_use]
    pub const fn status(mut self, status: AgentStatus) -> Self {
        self.initial_status = status;
        self
    }

    /// Sets the local heartbeat interval.
    #[must_use]
    pub const fn heartbeat_interval(mut self, heartbeat_interval: Duration) -> Self {
        self.heartbeat_interval = heartbeat_interval;
        self
    }

    /// Sets the registry TTL used to evict stale peers.
    #[must_use]
    pub const fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the lifecycle event broadcast channel capacity.
    #[must_use]
    pub const fn event_capacity(mut self, event_capacity: usize) -> Self {
        self.event_capacity = event_capacity;
        self
    }

    /// Adds or replaces a metadata entry.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Replaces the full metadata map.
    #[must_use]
    pub fn metadata_map(mut self, metadata: AgentMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Adds a typed capability to the local announcement.
    #[must_use]
    pub fn capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Replaces the full typed capability list for the local announcement.
    #[must_use]
    pub fn capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    /// Controls whether the local node announces itself on the LAN.
    #[must_use]
    pub const fn advertise_local(mut self, advertise_local: bool) -> Self {
        self.advertise_local = advertise_local;
        self
    }

    /// Builds a discovery-only node that browses peers without advertising itself.
    #[must_use]
    pub const fn discover_only(mut self) -> Self {
        self.advertise_local = false;
        self
    }

    /// Restricts the embedded mDNS daemon to a specific interface selector.
    #[must_use]
    pub fn enable_interface(mut self, interface: impl Into<NetworkInterface>) -> Self {
        self.enabled_interfaces.push(interface.into());
        self
    }

    /// Excludes a specific interface selector from the embedded mDNS daemon.
    #[must_use]
    pub fn disable_interface(mut self, interface: impl Into<NetworkInterface>) -> Self {
        self.disabled_interfaces.push(interface.into());
        self
    }

    /// Enables shared-secret signing and verification for mesh announcements.
    #[must_use]
    pub fn shared_secret(mut self, secret: impl Into<String>) -> Self {
        self.shared_secret_auth = Some(SharedSecretBuilderConfig {
            signing_secret: secret.into(),
            verification_secrets: Vec::new(),
            mode: SharedSecretMode::SignAndVerify,
        });
        self
    }

    /// Enables shared-secret authentication with an explicit mode.
    ///
    /// Invalid secrets are reported when [`Self::build`] is called.
    #[must_use]
    pub fn shared_secret_with_mode(
        mut self,
        secret: impl Into<String>,
        mode: SharedSecretMode,
    ) -> Self {
        self.shared_secret_auth = Some(SharedSecretBuilderConfig {
            signing_secret: secret.into(),
            verification_secrets: Vec::new(),
            mode,
        });
        self
    }

    /// Enables shared-secret authentication with rotation-aware verification.
    ///
    /// The signing secret is used for local announcements; the provided
    /// verification secrets are additionally accepted for incoming peers.
    #[must_use]
    pub fn shared_secret_rotation<I, S>(
        mut self,
        signing_secret: impl Into<String>,
        verification_secrets: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.shared_secret_auth = Some(SharedSecretBuilderConfig {
            signing_secret: signing_secret.into(),
            verification_secrets: verification_secrets.into_iter().map(Into::into).collect(),
            mode: SharedSecretMode::SignAndVerify,
        });
        self
    }

    /// Enables shared-secret authentication with rotation-aware verification and an explicit mode.
    #[must_use]
    pub fn shared_secret_rotation_with_mode<I, S>(
        mut self,
        signing_secret: impl Into<String>,
        verification_secrets: I,
        mode: SharedSecretMode,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.shared_secret_auth = Some(SharedSecretBuilderConfig {
            signing_secret: signing_secret.into(),
            verification_secrets: verification_secrets.into_iter().map(Into::into).collect(),
            mode,
        });
        self
    }

    /// Builds and starts a mesh runtime skeleton.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] if the configuration is invalid.
    pub async fn build(self) -> Result<ZeroConfMesh, ZeroConfError> {
        let config = self.build_config()?;
        ZeroConfMesh::from_config(config).await
    }

    fn build_config(self) -> Result<ZeroConfConfig, ZeroConfError> {
        let agent_id = self.agent_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let port = self.port.ok_or(ZeroConfError::InvalidPort)?;
        let capabilities = canonicalize_capabilities(self.capabilities)?;
        let metadata = self.metadata;

        let config = ZeroConfConfig::new(
            agent_id,
            self.role,
            self.project,
            self.branch,
            port,
            self.mdns_port,
            self.service_type,
            self.initial_status,
            self.heartbeat_interval,
            self.ttl,
            self.event_capacity,
            capabilities,
            metadata,
        )?;

        let config = config
            .with_advertise_local(self.advertise_local)
            .with_enabled_interfaces(self.enabled_interfaces)
            .with_disabled_interfaces(self.disabled_interfaces);

        if let Some(auth) = self.shared_secret_auth {
            Ok(
                config.with_shared_secret_auth(SharedSecretAuth::with_rotation(
                    auth.signing_secret,
                    auth.verification_secrets,
                    auth.mode,
                )?),
            )
        } else {
            Ok(config)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builder_should_generate_agent_id_when_missing() {
        let mesh = ZeroConfMesh::builder()
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(54_541)
            .build()
            .await
            .expect("builder should generate an agent id");

        assert!(!mesh.config().agent_id().is_empty());
        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn builder_should_reject_missing_port() {
        let err = ZeroConfMesh::builder()
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .build()
            .await
            .expect_err("missing port should be rejected");

        assert_eq!(err.to_string(), "port must be greater than zero");
    }

    #[tokio::test]
    async fn builder_should_reject_zero_event_capacity() {
        let err = ZeroConfMesh::builder()
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .event_capacity(0)
            .build()
            .await
            .expect_err("zero event capacity should be rejected");

        assert_eq!(err.to_string(), "event capacity must be greater than zero");
    }

    #[test]
    fn builder_should_embed_typed_capabilities_and_interface_policies_in_config() {
        let config = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .capability("review")
            .capability("plan")
            .enable_interface(NetworkInterface::LoopbackV4)
            .disable_interface(NetworkInterface::IPv6)
            .build_config()
            .expect("config should build");

        assert_eq!(
            config.capabilities(),
            &["plan".to_owned(), "review".to_owned()]
        );
        assert_eq!(config.enabled_interfaces(), &[NetworkInterface::LoopbackV4]);
        assert_eq!(config.disabled_interfaces(), &[NetworkInterface::IPv6]);
    }

    #[test]
    fn builder_should_reject_invalid_capabilities() {
        let err = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .capability("review,plan")
            .build_config()
            .expect_err("comma separated capability should be rejected");

        assert!(matches!(err, ZeroConfError::InvalidCapability { .. }));
    }

    #[test]
    fn builder_should_embed_shared_secret_auth_in_config() {
        let config = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .shared_secret("top-secret")
            .build_config()
            .expect("config should build");

        let auth = config
            .shared_secret_auth()
            .expect("shared secret auth should be present");
        assert_eq!(auth.mode(), SharedSecretMode::SignAndVerify);
        assert_eq!(auth.verification_secrets(), &["top-secret".to_owned()]);
    }

    #[test]
    fn builder_should_reject_empty_shared_secret() {
        let err = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .shared_secret("   ")
            .build_config()
            .expect_err("empty shared secret should be rejected");

        assert!(matches!(err, ZeroConfError::EmptySharedSecret));
    }

    #[test]
    fn builder_should_embed_rotation_aware_shared_secret_auth_in_config() {
        let config = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .shared_secret_rotation("new-secret", ["old-secret"])
            .build_config()
            .expect("config should build");

        let auth = config
            .shared_secret_auth()
            .expect("shared secret auth should be present");
        assert_eq!(auth.mode(), SharedSecretMode::SignAndVerify);
        assert_eq!(
            auth.verification_secrets(),
            &["new-secret".to_owned(), "old-secret".to_owned()]
        );
    }

    #[test]
    fn builder_should_reject_reserved_metadata_keys() {
        let err = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .metadata(crate::AGENT_SIGNATURE_METADATA_KEY, "forged")
            .build_config()
            .expect_err("reserved metadata should be rejected");

        assert!(matches!(err, ZeroConfError::ReservedMetadataKey { key } if key == "zcm_sig"));
    }

    #[test]
    fn builder_should_support_discovery_only_mode() {
        let config = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .discover_only()
            .build_config()
            .expect("config should build");

        assert!(!config.advertise_local());
    }
}
