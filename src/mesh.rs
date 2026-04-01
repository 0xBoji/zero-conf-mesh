use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use tokio::{
    sync::{RwLock, watch},
    task::JoinHandle,
    time,
};
use tracing::debug;

use crate::{
    builder::ZeroConfMeshBuilder,
    config::ZeroConfConfig,
    error::ZeroConfError,
    events::AgentEvent,
    registry::Registry,
    types::{AgentAnnouncement, AgentStatus},
};

/// High-level runtime handle for the local mesh node.
#[derive(Debug)]
pub struct ZeroConfMesh {
    config: ZeroConfConfig,
    registry: Registry,
    local_agent: std::sync::Arc<RwLock<AgentAnnouncement>>,
    shutdown_tx: watch::Sender<bool>,
    heartbeat_task: Mutex<Option<JoinHandle<()>>>,
    sweeper_task: Mutex<Option<JoinHandle<()>>>,
    shutdown_requested: AtomicBool,
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
        let registry = Registry::new(config.ttl());
        let local_announcement = config.local_announcement()?;
        let local_agent = std::sync::Arc::new(RwLock::new(local_announcement.clone()));
        registry.upsert(local_announcement).await;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let heartbeat_task = spawn_heartbeat_task(
            registry.clone(),
            local_agent.clone(),
            config.heartbeat_interval(),
            shutdown_rx.clone(),
        );
        let sweeper_task =
            spawn_sweeper_task(registry.clone(), config.heartbeat_interval(), shutdown_rx);

        Ok(Self {
            config,
            registry,
            local_agent,
            shutdown_tx,
            heartbeat_task: Mutex::new(Some(heartbeat_task)),
            sweeper_task: Mutex::new(Some(sweeper_task)),
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

    /// Subscribes to registry lifecycle events.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
        self.registry.subscribe()
    }

    /// Updates the local agent status and refreshes the registry entry immediately.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the local announcement becomes invalid.
    pub async fn update_status(&self, status: AgentStatus) -> Result<(), ZeroConfError> {
        {
            let mut local_agent = self.local_agent.write().await;
            local_agent.set_status(status);
        }

        self.refresh_local_agent().await
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
        let local_agent_id = {
            let local_agent = self.local_agent.read().await;
            local_agent.agent_id().to_owned()
        };
        let _ = self.registry.remove(&local_agent_id).await;

        if let Some(handle) = take_task(&self.heartbeat_task) {
            handle.await?;
        }

        if let Some(handle) = take_task(&self.sweeper_task) {
            handle.await?;
        }

        Ok(())
    }

    async fn refresh_local_agent(&self) -> Result<(), ZeroConfError> {
        let announcement = self.local_agent.read().await.clone();
        self.registry.upsert(announcement).await;
        Ok(())
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
    }
}

fn spawn_heartbeat_task(
    registry: Registry,
    local_agent: std::sync::Arc<RwLock<AgentAnnouncement>>,
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
                    let _ = registry.upsert(announcement).await;
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
    use std::time::Duration;

    use tokio::time;

    use super::*;

    #[tokio::test]
    async fn mesh_should_update_local_status() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .port(8080)
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
    async fn mesh_shutdown_should_remove_local_agent() {
        let mesh = ZeroConfMesh::builder()
            .agent_id("agent-1")
            .role("coder")
            .project("alpha")
            .port(8080)
            .build()
            .await
            .expect("mesh should build");

        mesh.shutdown().await.expect("shutdown should succeed");

        time::sleep(Duration::from_millis(10)).await;
        let agent = mesh.registry().get("agent-1").await;
        assert!(agent.is_none());
    }
}
