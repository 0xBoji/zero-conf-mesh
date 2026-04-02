use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use mdns_sd::ServiceDaemon;
use tokio::{
    sync::{RwLock, watch},
    task::JoinHandle,
    time,
};
use tracing::{debug, warn};

use crate::{
    broadcaster::Broadcaster,
    builder::ZeroConfMeshBuilder,
    config::ZeroConfConfig,
    error::ZeroConfError,
    events::AgentEvent,
    listener::Listener,
    registry::Registry,
    types::{AgentAnnouncement, AgentInfo, AgentStatus},
};

/// High-level runtime handle for the local mesh node.
///
/// # Example
/// ```no_run
/// use coding_agent_mesh_presence::{AgentStatus, ZeroConfMesh};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mesh = ZeroConfMesh::builder()
///     .agent_id("agent-01")
///     .role("worker")
///     .project("alpha")
///     .branch("main")
///     .port(8080)
///     .build()
///     .await?;
///
/// mesh.update_status(AgentStatus::Busy).await?;
/// let local = mesh.local_agent().await;
/// assert_eq!(local.agent_id(), "agent-01");
/// mesh.shutdown().await?;
/// # Ok(())
/// # }
/// ```
pub struct ZeroConfMesh {
    config: ZeroConfConfig,
    registry: Registry,
    local_agent: std::sync::Arc<RwLock<AgentAnnouncement>>,
    broadcaster: Broadcaster,
    daemon: ServiceDaemon,
    shutdown_tx: watch::Sender<bool>,
    heartbeat_task: Mutex<Option<JoinHandle<()>>>,
    sweeper_task: Mutex<Option<JoinHandle<()>>>,
    listener_task: Mutex<Option<JoinHandle<()>>>,
    shutdown_requested: AtomicBool,
}

impl std::fmt::Debug for ZeroConfMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZeroConfMesh")
            .field("config", &self.config)
            .field("registry", &self.registry)
            .field(
                "shutdown_requested",
                &self.shutdown_requested.load(Ordering::Acquire),
            )
            .finish_non_exhaustive()
    }
}

impl ZeroConfMesh {
    /// Starts building a new mesh instance.
    #[must_use]
    pub fn builder() -> ZeroConfMeshBuilder {
        ZeroConfMeshBuilder::default()
    }

    /// Creates a running mesh from an already validated config.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the runtime cannot be initialized.
    pub async fn from_config(config: ZeroConfConfig) -> Result<Self, ZeroConfError> {
        let registry = Registry::with_event_capacity(config.ttl(), config.event_capacity());
        let mut local_announcement = config.local_announcement()?;
        if let Some(auth) = config.shared_secret_auth() {
            local_announcement.apply_shared_secret_auth(auth);
        }
        let local_agent = std::sync::Arc::new(RwLock::new(local_announcement.clone()));
        let daemon = ServiceDaemon::new_with_port(config.mdns_port())?;

        for interface in config.enabled_interfaces() {
            daemon.enable_interface(interface.to_mdns_if_kind())?;
        }

        for interface in config.disabled_interfaces() {
            daemon.disable_interface(interface.to_mdns_if_kind())?;
        }

        let broadcaster =
            Broadcaster::new(daemon.clone(), config.service_type(), config.host_name());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let listener = Listener::new(
            daemon.clone(),
            config.service_type(),
            config.agent_id(),
            config.instance_name(),
            config.shared_secret_auth().cloned(),
        );
        let listener_task = if config.advertise_local() {
            announce_and_start_listener(
                &broadcaster,
                &daemon,
                &local_announcement,
                listener,
                registry.clone(),
                shutdown_rx.clone(),
                Listener::spawn,
            )
            .await?
        } else {
            Listener::spawn(listener, registry.clone(), shutdown_rx.clone())?
        };

        if config.advertise_local() {
            registry.upsert_local(local_announcement).await;
        }

        let heartbeat_task = if config.advertise_local() {
            Some(spawn_heartbeat_task(
                registry.clone(),
                local_agent.clone(),
                broadcaster.clone(),
                config.heartbeat_interval(),
                shutdown_rx.clone(),
            ))
        } else {
            None
        };
        let sweeper_task =
            spawn_sweeper_task(registry.clone(), config.heartbeat_interval(), shutdown_rx);

        Ok(Self {
            config,
            registry,
            local_agent,
            broadcaster,
            daemon,
            shutdown_tx,
            heartbeat_task: Mutex::new(heartbeat_task),
            sweeper_task: Mutex::new(Some(sweeper_task)),
            listener_task: Mutex::new(Some(listener_task)),
            shutdown_requested: AtomicBool::new(false),
        })
    }

    /// Returns the immutable runtime configuration.
    #[must_use]
    pub const fn config(&self) -> &ZeroConfConfig {
        &self.config
    }

    /// Returns the shared registry handle.
    #[must_use]
    pub const fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Returns the local agent identifier.
    #[must_use]
    pub fn local_agent_id(&self) -> &str {
        self.config.agent_id()
    }

    /// Returns a snapshot of the local agent announcement.
    ///
    /// This snapshot is useful when you want the exact metadata currently being
    /// advertised by the local node.
    pub async fn local_agent(&self) -> AgentAnnouncement {
        self.local_agent.read().await.clone()
    }

    /// Returns a single agent by identifier from the registry.
    pub async fn get_agent(&self, agent_id: &str) -> Option<AgentInfo> {
        self.registry.get(agent_id).await
    }

    /// Returns all known agents from the registry.
    pub async fn agents(&self) -> Vec<AgentInfo> {
        self.registry.list().await
    }

    /// Returns all known agents for a specific project namespace.
    pub async fn agents_by_project(&self, project: &str) -> Vec<AgentInfo> {
        self.registry.get_all_by_project(project).await
    }

    /// Returns all known agents for a specific branch or workstream.
    pub async fn agents_by_branch(&self, branch: &str) -> Vec<AgentInfo> {
        self.registry.get_all_by_branch(branch).await
    }

    /// Returns all known agents matching both project and branch.
    pub async fn agents_by_project_and_branch(
        &self,
        project: &str,
        branch: &str,
    ) -> Vec<AgentInfo> {
        self.registry
            .get_all_by_project_and_branch(project, branch)
            .await
    }

    /// Returns all known agents matching a specific status.
    pub async fn agents_by_status(&self, status: AgentStatus) -> Vec<AgentInfo> {
        self.registry.get_all_by_status(status).await
    }

    /// Returns all known agents matching a specific role.
    pub async fn agents_by_role(&self, role: &str) -> Vec<AgentInfo> {
        self.registry.get_all_by_role(role).await
    }

    /// Returns all known agents that contain the provided metadata key.
    pub async fn agents_with_metadata_key(&self, key: &str) -> Vec<AgentInfo> {
        self.registry.get_all_with_metadata_key(key).await
    }

    /// Returns all known agents whose metadata contains the provided key/value pair.
    pub async fn agents_with_metadata(&self, key: &str, value: &str) -> Vec<AgentInfo> {
        self.registry.get_all_by_metadata(key, value).await
    }

    /// Returns all known agents that contain a metadata key with the provided prefix.
    pub async fn agents_with_metadata_key_prefix(&self, prefix: &str) -> Vec<AgentInfo> {
        self.registry.get_all_with_metadata_key_prefix(prefix).await
    }

    /// Returns all known agents whose metadata value starts with the provided prefix.
    pub async fn agents_with_metadata_prefix(&self, key: &str, prefix: &str) -> Vec<AgentInfo> {
        self.registry.get_all_by_metadata_prefix(key, prefix).await
    }

    /// Returns all known agents whose metadata value matches the provided regex.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the regex pattern is invalid.
    pub async fn agents_with_metadata_regex(
        &self,
        key: &str,
        pattern: &str,
    ) -> Result<Vec<AgentInfo>, ZeroConfError> {
        self.registry.get_all_by_metadata_regex(key, pattern).await
    }

    /// Returns all known agents that advertise the provided typed capability.
    pub async fn agents_with_capability(&self, capability: &str) -> Vec<AgentInfo> {
        self.registry.get_all_with_capability(capability).await
    }

    /// Returns all known agents matching a custom predicate.
    pub async fn query_agents<F>(&self, predicate: F) -> Vec<AgentInfo>
    where
        F: Fn(&AgentInfo) -> bool,
    {
        self.registry.query(predicate).await
    }

    /// Convenience alias for branch-focused queries.
    pub async fn who_is_on_branch(&self, branch: &str) -> Vec<AgentInfo> {
        self.agents_by_branch(branch).await
    }

    /// Subscribes to registry lifecycle events.
    ///
    /// # Example
    /// ```no_run
    /// use coding_agent_mesh_presence::{AgentEvent, ZeroConfMesh};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mesh = ZeroConfMesh::builder()
    ///     .agent_id("agent-01")
    ///     .role("worker")
    ///     .project("alpha")
    ///     .branch("main")
    ///     .port(8080)
    ///     .build()
    ///     .await?;
    ///
    /// let mut events = mesh.subscribe();
    /// tokio::spawn(async move {
    ///     while let Ok(event) = events.recv().await {
    ///         match event {
    ///             AgentEvent::Joined { .. }
    ///             | AgentEvent::Updated { .. }
    ///             | AgentEvent::Left { .. }
    ///             | _ => {}
    ///         }
    ///     }
    /// });
    ///
    /// mesh.shutdown().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
        self.registry.subscribe()
    }

    /// Updates the local agent status and refreshes the registry entry immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the local announcement becomes invalid.
    pub async fn update_status(&self, status: AgentStatus) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| {
            local_agent.set_status(status);
            Ok(())
        })
        .await
    }

    /// Updates the local project namespace and refreshes the announcement immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the provided project is empty after trimming.
    pub async fn update_project(&self, project: impl Into<String>) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| local_agent.set_project(project))
            .await
    }

    /// Updates the local branch/workstream and refreshes the announcement immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the provided branch is empty after trimming.
    pub async fn update_branch(&self, branch: impl Into<String>) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| local_agent.set_branch(branch))
            .await
    }

    /// Updates a non-canonical metadata entry and refreshes the announcement immediately.
    ///
    /// Canonical keys such as `status`, `current_project`, and `current_branch` are
    /// managed by dedicated runtime updaters and will be rejected here.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the key is empty or reserved by the crate.
    pub async fn update_metadata(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| local_agent.set_metadata(key, value))
            .await
    }

    /// Removes a non-canonical metadata entry and refreshes the announcement immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the key is empty or reserved by the crate.
    pub async fn remove_metadata(&self, key: impl Into<String>) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| local_agent.remove_metadata(key))
            .await
    }

    /// Replaces the local typed capability list and refreshes the announcement immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when any capability is empty or contains a comma.
    pub async fn update_capabilities<I, S>(&self, capabilities: I) -> Result<(), ZeroConfError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.mutate_local_agent(|local_agent| local_agent.set_capabilities(capabilities))
            .await
    }

    /// Adds a typed capability to the local announcement and refreshes immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the capability is empty or contains a comma.
    pub async fn add_capability(&self, capability: impl Into<String>) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| local_agent.add_capability(capability))
            .await
    }

    /// Removes a typed capability from the local announcement and refreshes immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the capability is empty after trimming.
    pub async fn remove_capability(
        &self,
        capability: impl Into<String>,
    ) -> Result<(), ZeroConfError> {
        self.mutate_local_agent(|local_agent| local_agent.remove_capability(capability))
            .await
    }

    /// Gracefully stops background tasks and removes the local agent from the registry.
    ///
    /// Calling this method multiple times is safe.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when a background task fails to join.
    pub async fn shutdown(&self) -> Result<(), ZeroConfError> {
        if self.shutdown_requested.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let _ = self.shutdown_tx.send(true);
        let local_announcement = self.local_agent.read().await.clone();

        if self.config.advertise_local() {
            if let Err(error) = self.broadcaster.unregister(&local_announcement).await {
                warn!(?error, "failed to unregister local service");
            }

            let _ = self
                .registry
                .remove_local(local_announcement.agent_id())
                .await;
        }

        if let Some(handle) = take_task(&self.heartbeat_task) {
            handle.await?;
        }

        if let Some(handle) = take_task(&self.sweeper_task) {
            handle.await?;
        }

        if let Some(handle) = take_task(&self.listener_task) {
            handle.await?;
        }

        let _ = self.daemon.shutdown();

        Ok(())
    }

    async fn refresh_local_agent(&self) -> Result<(), ZeroConfError> {
        if !self.config.advertise_local() {
            return Ok(());
        }

        let announcement = {
            let mut local_agent = self.local_agent.write().await;
            if let Some(auth) = self.config.shared_secret_auth() {
                local_agent.apply_shared_secret_auth(auth);
            }
            local_agent.clone()
        };
        self.broadcaster.announce(&announcement)?;
        self.registry.upsert_local(announcement).await;
        Ok(())
    }

    async fn mutate_local_agent<F>(&self, mutator: F) -> Result<(), ZeroConfError>
    where
        F: FnOnce(&mut AgentAnnouncement) -> Result<(), ZeroConfError>,
    {
        {
            let mut local_agent = self.local_agent.write().await;
            mutator(&mut local_agent)?;
        }

        self.refresh_local_agent().await
    }
}

impl Drop for ZeroConfMesh {
    fn drop(&mut self) {
        if self.shutdown_requested.swap(true, Ordering::AcqRel) {
            return;
        }

        let _ = self.shutdown_tx.send(true);

        if let Ok(mut handle) = self.heartbeat_task.lock()
            && let Some(handle) = handle.take()
        {
            handle.abort();
        }

        if let Ok(mut handle) = self.sweeper_task.lock()
            && let Some(handle) = handle.take()
        {
            handle.abort();
        }

        if let Ok(mut handle) = self.listener_task.lock()
            && let Some(handle) = handle.take()
        {
            handle.abort();
        }

        let _ = self.daemon.shutdown();
    }
}

async fn announce_and_start_listener<F>(
    broadcaster: &Broadcaster,
    daemon: &ServiceDaemon,
    local_announcement: &AgentAnnouncement,
    listener: Listener,
    registry: Registry,
    shutdown_rx: watch::Receiver<bool>,
    start_listener: F,
) -> Result<JoinHandle<()>, ZeroConfError>
where
    F: FnOnce(Listener, Registry, watch::Receiver<bool>) -> Result<JoinHandle<()>, ZeroConfError>,
{
    broadcaster.announce(local_announcement)?;

    match start_listener(listener, registry, shutdown_rx) {
        Ok(task) => Ok(task),
        Err(error) => {
            let _ = broadcaster.unregister(local_announcement).await;
            let _ = daemon.shutdown();
            Err(error)
        }
    }
}

fn spawn_heartbeat_task(
    registry: Registry,
    local_agent: std::sync::Arc<RwLock<AgentAnnouncement>>,
    broadcaster: Broadcaster,
    heartbeat_interval: std::time::Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = time::interval(heartbeat_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                _ = interval.tick() => {
                    let announcement = local_agent.read().await.clone();
                    if let Err(error) = broadcaster.announce(&announcement) {
                        warn!(?error, "failed to re-announce local service");
                    }
                    let _ = registry.upsert_local(announcement).await;
                }
            }
        }
    })
}

fn spawn_sweeper_task(
    registry: Registry,
    sweep_interval: std::time::Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = time::interval(sweep_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                _ = interval.tick() => {
                    let evicted = registry.evict_stale().await;
                    if !evicted.is_empty() {
                        debug!(evicted = evicted.len(), "evicted stale agents from registry");
                    }
                }
            }
        }
    })
}

fn take_task(task: &Mutex<Option<JoinHandle<()>>>) -> Option<JoinHandle<()>> {
    task.lock().ok().and_then(|mut handle| handle.take())
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, UdpSocket},
        time::Duration,
    };

    use mdns_sd::ServiceEvent;
    use tokio::time;

    use super::*;
    use crate::{DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_TTL, NetworkInterface, ZeroConfConfig};

    #[tokio::test]
    async fn mesh_should_update_local_status() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .heartbeat_interval(Duration::from_secs(30))
            .ttl(Duration::from_secs(120))
            .build()
            .await
            .expect("mesh should build");

        mesh.update_status(AgentStatus::Busy)
            .await
            .expect("status update should succeed");

        let agent = mesh
            .registry()
            .get("agent-1")
            .await
            .expect("local agent should stay registered");

        assert_eq!(agent.status(), AgentStatus::Busy);
        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_startup_should_cleanup_local_registration_when_listener_fails() {
        let mdns_port = available_udp_port();
        let service_type = "_mesh-startup-cleanup._tcp.local.";
        let config = ZeroConfConfig::new(
            "agent-cleanup",
            "coder",
            "alpha",
            "main",
            8080,
            mdns_port,
            service_type,
            AgentStatus::Idle,
            DEFAULT_HEARTBEAT_INTERVAL,
            DEFAULT_TTL,
            crate::DEFAULT_EVENT_CAPACITY,
            Vec::new(),
            crate::AgentMetadata::new(),
        )
        .expect("config should be valid");
        let registry = Registry::with_event_capacity(config.ttl(), config.event_capacity());
        let local_announcement = config
            .local_announcement()
            .expect("local announcement should build");
        let daemon = ServiceDaemon::new_with_port(config.mdns_port())
            .expect("daemon should bind to test mdns port");
        let broadcaster =
            Broadcaster::new(daemon.clone(), config.service_type(), config.host_name());
        let listener = Listener::new(
            daemon.clone(),
            config.service_type(),
            config.agent_id(),
            config.instance_name(),
            config.shared_secret_auth().cloned(),
        );
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        let err = announce_and_start_listener(
            &broadcaster,
            &daemon,
            &local_announcement,
            listener,
            registry,
            shutdown_rx,
            |_listener, _registry, _shutdown_rx| {
                Err(ZeroConfError::Mdns(mdns_sd::Error::Msg(
                    "injected listener failure".to_owned(),
                )))
            },
        )
        .await
        .expect_err("startup should fail when listener startup fails");

        assert!(matches!(
            err,
            ZeroConfError::Mdns(mdns_sd::Error::Msg(message))
            if message == "injected listener failure"
        ));
        assert!(
            !service_should_resolve_on_network(
                config.service_type(),
                local_announcement.agent_id(),
                local_announcement.instance_name(),
                mdns_port,
            )
            .await,
            "cleanup should remove the partially announced local service"
        );
    }

    #[tokio::test]
    async fn mesh_should_update_local_project_branch_and_metadata() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .build()
            .await
            .expect("mesh should build");

        mesh.update_project("beta")
            .await
            .expect("project update should succeed");
        mesh.update_branch("feature/runtime")
            .await
            .expect("branch update should succeed");
        mesh.update_metadata("capability", "planning")
            .await
            .expect("metadata update should succeed");

        let local = mesh.local_agent().await;
        let beta_agents = mesh.agents_by_project("beta").await;
        let alpha_agents = mesh.agents_by_project("alpha").await;
        let branch_agents = mesh.agents_by_branch("feature/runtime").await;

        assert_eq!(local.project(), "beta");
        assert_eq!(local.branch(), "feature/runtime");
        assert_eq!(
            local.metadata().get("capability"),
            Some(&"planning".to_owned())
        );
        assert_eq!(beta_agents.len(), 1);
        assert_eq!(alpha_agents.len(), 0);
        assert_eq!(branch_agents.len(), 1);

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_should_remove_runtime_metadata() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .metadata("capability", "planning")
            .build()
            .await
            .expect("mesh should build");

        mesh.remove_metadata("capability")
            .await
            .expect("metadata removal should succeed");

        let local = mesh.local_agent().await;
        let matches = mesh.agents_with_metadata_key("capability").await;

        assert_eq!(local.metadata().get("capability"), None);
        assert!(matches.is_empty());

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_should_manage_typed_capabilities() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .capabilities(["review", "plan"])
            .build()
            .await
            .expect("mesh should build");

        mesh.add_capability("debug")
            .await
            .expect("capability should be added");
        mesh.remove_capability("plan")
            .await
            .expect("capability should be removed");

        let local = mesh.local_agent().await;
        let reviewers = mesh.agents_with_capability("review").await;

        assert_eq!(
            local.capabilities(),
            &["debug".to_owned(), "review".to_owned()]
        );
        assert_eq!(reviewers.len(), 1);
        assert_eq!(
            local.metadata().get(crate::AGENT_CAPABILITIES_METADATA_KEY),
            Some(&"debug,review".to_owned())
        );

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_should_reject_reserved_metadata_updates() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .build()
            .await
            .expect("mesh should build");

        let err = mesh
            .update_metadata(crate::AGENT_STATUS_METADATA_KEY, "busy")
            .await
            .expect_err("reserved metadata keys should be rejected");

        assert!(matches!(
            err,
            ZeroConfError::ReservedMetadataKey { key } if key == "status"
        ));

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_should_expose_high_level_agent_queries() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .metadata("capability", "review")
            .capabilities(["review", "plan"])
            .build()
            .await
            .expect("mesh should build");

        let local = mesh.local_agent().await;
        let agents = mesh.agents().await;
        let alpha = mesh.agents_by_project("alpha").await;
        let main = mesh.agents_by_branch("main").await;
        let alpha_main = mesh.agents_by_project_and_branch("alpha", "main").await;
        let idle = mesh.agents_by_status(AgentStatus::Idle).await;
        let coders = mesh.agents_by_role("coder").await;
        let key_prefix = mesh.agents_with_metadata_key_prefix("cap").await;
        let value_prefix = mesh.agents_with_metadata_prefix("capability", "rev").await;
        let regex = mesh
            .agents_with_metadata_regex("capability", "rev(iew)?")
            .await
            .expect("regex should compile");
        let capability_agents = mesh.agents_with_capability("plan").await;
        let custom = mesh
            .query_agents(|agent| agent.project() == "alpha" && agent.has_capability("review"))
            .await;

        assert_eq!(mesh.local_agent_id(), "agent-1");
        assert_eq!(local.agent_id(), "agent-1");
        assert_eq!(local.branch(), "main");
        assert_eq!(agents.len(), 1);
        assert_eq!(alpha.len(), 1);
        assert_eq!(main.len(), 1);
        assert_eq!(alpha_main.len(), 1);
        assert_eq!(idle.len(), 1);
        assert_eq!(coders.len(), 1);
        assert_eq!(key_prefix.len(), 1);
        assert_eq!(value_prefix.len(), 1);
        assert_eq!(regex.len(), 1);
        assert_eq!(capability_agents.len(), 1);
        assert_eq!(custom.len(), 1);

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_should_apply_interface_policy_from_builder() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .enable_interface(NetworkInterface::LoopbackV4)
            .disable_interface(NetworkInterface::IPv6)
            .build()
            .await
            .expect("mesh should build");

        assert_eq!(
            mesh.config().enabled_interfaces(),
            &[NetworkInterface::LoopbackV4]
        );
        assert_eq!(
            mesh.config().disabled_interfaces(),
            &[NetworkInterface::IPv6]
        );

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_should_support_discovery_only_mode_without_local_registry_entry() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-observer")
            .role("observer")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .discover_only()
            .build()
            .await
            .expect("mesh should build");

        assert!(mesh.registry().get("agent-observer").await.is_none());
        assert!(mesh.agents().await.is_empty());

        mesh.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn mesh_shutdown_should_remove_local_agent() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .build()
            .await
            .expect("mesh should build");

        mesh.shutdown().await.expect("shutdown should succeed");

        time::sleep(Duration::from_millis(10)).await;
        let agent = mesh.registry().get("agent-1").await;
        assert!(agent.is_none());
    }

    #[tokio::test]
    async fn mesh_should_discover_peer_on_custom_mdns_port() {
        let mdns_port = available_udp_port();
        let mesh_a = ZeroConfMesh::builder()
            .agent_id("agent-a")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8081)
            .mdns_port(mdns_port)
            .heartbeat_interval(Duration::from_millis(200))
            .ttl(Duration::from_secs(2))
            .metadata("capability", "implementation")
            .build()
            .await
            .expect("mesh a should build");

        let mesh_b = ZeroConfMesh::builder()
            .agent_id("agent-b")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8082)
            .mdns_port(mdns_port)
            .heartbeat_interval(Duration::from_millis(200))
            .ttl(Duration::from_secs(2))
            .metadata("capability", "review")
            .build()
            .await
            .expect("mesh b should build");

        let deadline = time::Instant::now() + Duration::from_secs(5);
        let mut discovered = false;
        while time::Instant::now() < deadline {
            if mesh_a.registry().get("agent-b").await.is_some()
                && mesh_b.registry().get("agent-a").await.is_some()
            {
                discovered = true;
                break;
            }

            time::sleep(Duration::from_millis(100)).await;
        }

        mesh_a
            .shutdown()
            .await
            .expect("mesh a shutdown should succeed");
        mesh_b
            .shutdown()
            .await
            .expect("mesh b shutdown should succeed");

        assert!(discovered, "both peers should discover each other");
    }

    #[tokio::test]
    async fn mesh_should_propagate_custom_metadata_across_discovery() {
        let mdns_port = available_udp_port();
        let mesh_a = ZeroConfMesh::builder()
            .agent_id("agent-a")
            .role("coder")
            .project("alpha")
            .branch("feature/mesh")
            .port(8081)
            .mdns_port(mdns_port)
            .heartbeat_interval(Duration::from_millis(200))
            .ttl(Duration::from_secs(2))
            .metadata("capability", "implementation")
            .build()
            .await
            .expect("mesh a should build");

        let mesh_b = ZeroConfMesh::builder()
            .agent_id("agent-b")
            .role("reviewer")
            .project("alpha")
            .branch("main")
            .port(8082)
            .mdns_port(mdns_port)
            .heartbeat_interval(Duration::from_millis(200))
            .ttl(Duration::from_secs(2))
            .metadata("capability", "review")
            .build()
            .await
            .expect("mesh b should build");

        let peer = wait_for_agent(&mesh_a, "agent-b").await;

        mesh_a
            .shutdown()
            .await
            .expect("mesh a shutdown should succeed");
        mesh_b
            .shutdown()
            .await
            .expect("mesh b shutdown should succeed");

        let peer = peer.expect("agent-b should be discovered");
        assert_eq!(
            peer.metadata().get("capability"),
            Some(&"review".to_owned())
        );
        assert_eq!(peer.branch(), "main");
    }

    #[tokio::test]
    async fn mesh_shutdown_should_emit_local_update_then_local_left_in_order() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .branch("main")
            .port(8080)
            .mdns_port(available_udp_port())
            .build()
            .await
            .expect("mesh should build");

        let mut events = mesh.subscribe();

        mesh.update_status(AgentStatus::Busy)
            .await
            .expect("status update should succeed");
        mesh.shutdown().await.expect("shutdown should succeed");

        let first = events
            .recv()
            .await
            .expect("first event should be available");
        let second = events
            .recv()
            .await
            .expect("second event should be available");

        assert!(matches!(
            first,
            AgentEvent::Updated {
                origin: crate::EventOrigin::Local,
                ..
            }
        ));
        assert!(matches!(
            second,
            AgentEvent::Left {
                origin: crate::EventOrigin::Local,
                reason: crate::DepartureReason::Graceful,
                ..
            }
        ));
    }

    fn available_udp_port() -> u16 {
        UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("ephemeral UDP port should be allocated")
            .local_addr()
            .expect("local address should be available")
            .port()
    }

    async fn service_should_resolve_on_network(
        service_type: &str,
        instance_name: &str,
        fullname: &str,
        mdns_port: u16,
    ) -> bool {
        let observer = ServiceDaemon::new_with_port(mdns_port)
            .expect("observer daemon should bind to test mdns port");
        let receiver = observer
            .browse(service_type)
            .expect("observer should browse service type");

        let resolved = wait_for_resolved_service(receiver, instance_name, fullname).await;

        let _ = observer.stop_browse(service_type);
        let _ = observer.shutdown();

        resolved
    }

    async fn wait_for_resolved_service(
        receiver: mdns_sd::Receiver<ServiceEvent>,
        instance_name: &str,
        fullname: &str,
    ) -> bool {
        let deadline = time::Instant::now() + Duration::from_secs(2);
        while time::Instant::now() < deadline {
            match time::timeout(Duration::from_millis(200), receiver.recv_async()).await {
                Ok(Ok(ServiceEvent::ServiceResolved(service)))
                    if service.get_fullname() == fullname
                        || service.get_fullname() == instance_name =>
                {
                    return true;
                }
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {}
            }
        }

        false
    }

    async fn wait_for_agent(mesh: &ZeroConfMesh, agent_id: &str) -> Option<AgentInfo> {
        let deadline = time::Instant::now() + Duration::from_secs(5);
        while time::Instant::now() < deadline {
            if let Some(agent) = mesh.get_agent(agent_id).await {
                return Some(agent);
            }

            time::sleep(Duration::from_millis(100)).await;
        }

        None
    }
}
