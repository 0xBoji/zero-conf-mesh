use std::time::Duration;

use uuid::Uuid;

use crate::{
    config::{DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_SERVICE_TYPE, DEFAULT_TTL, ZeroConfConfig},
    error::ZeroConfError,
    mesh::ZeroConfMesh,
    types::{AgentMetadata, AgentStatus},
};

/// Builder for constructing a [`ZeroConfMesh`] instance.
#[derive(Debug, Clone)]
pub struct ZeroConfMeshBuilder {
    agent_id: Option<String>,
    role: String,
    project: String,
    port: Option<u16>,
    service_type: String,
    initial_status: AgentStatus,
    heartbeat_interval: Duration,
    ttl: Duration,
    metadata: AgentMetadata,
}

impl Default for ZeroConfMeshBuilder {
    fn default() -> Self {
        Self {
            agent_id: None,
            role: "agent".to_owned(),
            project: "default".to_owned(),
            port: None,
            service_type: DEFAULT_SERVICE_TYPE.to_owned(),
            initial_status: AgentStatus::Idle,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            ttl: DEFAULT_TTL,
            metadata: AgentMetadata::new(),
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

    /// Sets the advertised service port.
    #[must_use]
    pub const fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
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

        ZeroConfConfig::new(
            agent_id,
            self.role,
            self.project,
            port,
            self.service_type,
            self.initial_status,
            self.heartbeat_interval,
            self.ttl,
            self.metadata,
        )
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
            .port(8080)
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
            .build()
            .await
            .expect_err("missing port should be rejected");

        assert_eq!(err.to_string(), "port must be greater than zero");
    }
}
