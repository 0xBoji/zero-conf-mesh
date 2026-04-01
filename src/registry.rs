use std::{collections::HashMap, sync::Arc, time::Instant};

use tokio::sync::{RwLock, broadcast};

use crate::{
    events::{AgentEvent, DepartureReason, EventOrigin},
    types::{AgentAnnouncement, AgentInfo},
};

const DEFAULT_EVENT_CAPACITY: usize = 256;

/// Result of an upsert operation against the registry.
#[derive(Debug, Clone)]
pub enum RegistryUpsert {
    /// A new agent was inserted.
    Inserted(AgentInfo),
    /// An existing agent changed one or more advertised fields.
    Updated {
        /// The previous state.
        previous: AgentInfo,
        /// The current state.
        current: AgentInfo,
    },
    /// An existing agent only refreshed its last-seen timestamp.
    Refreshed(AgentInfo),
}

/// Concurrent in-memory registry of discovered agents.
#[derive(Debug, Clone)]
pub struct Registry {
    inner: Arc<RwLock<HashMap<String, AgentInfo>>>,
    ttl: std::time::Duration,
    events_tx: broadcast::Sender<AgentEvent>,
}

impl Registry {
    /// Creates a new registry with the provided TTL.
    #[must_use]
    pub fn new(ttl: std::time::Duration) -> Self {
        Self::with_event_capacity(ttl, DEFAULT_EVENT_CAPACITY)
    }

    /// Creates a new registry with a custom broadcast event capacity.
    #[must_use]
    pub fn with_event_capacity(ttl: std::time::Duration, event_capacity: usize) -> Self {
        let (events_tx, _) = broadcast::channel(event_capacity);
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            ttl,
            events_tx,
        }
    }

    /// Returns the configured TTL.
    #[must_use]
    pub const fn ttl(&self) -> std::time::Duration {
        self.ttl
    }

    /// Subscribes to registry lifecycle events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.events_tx.subscribe()
    }

    /// Inserts or updates an agent based on its unique identifier.
    pub async fn upsert(&self, announcement: AgentAnnouncement) -> RegistryUpsert {
        self.upsert_remote(announcement).await
    }

    /// Inserts or updates the local agent and emits local-origin events.
    pub async fn upsert_local(&self, announcement: AgentAnnouncement) -> RegistryUpsert {
        self.upsert_with_origin_at(announcement, Instant::now(), EventOrigin::Local)
            .await
    }

    /// Inserts or updates a remote agent and emits remote-origin events.
    pub async fn upsert_remote(&self, announcement: AgentAnnouncement) -> RegistryUpsert {
        self.upsert_with_origin_at(announcement, Instant::now(), EventOrigin::Remote)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn upsert_at(
        &self,
        announcement: AgentAnnouncement,
        seen_at: Instant,
    ) -> RegistryUpsert {
        self.upsert_with_origin_at(announcement, seen_at, EventOrigin::Remote)
            .await
    }

    pub(crate) async fn upsert_with_origin_at(
        &self,
        announcement: AgentAnnouncement,
        seen_at: Instant,
        origin: EventOrigin,
    ) -> RegistryUpsert {
        let incoming = announcement.into_agent_info(seen_at);
        let mut registry = self.inner.write().await;

        match registry.get_mut(incoming.id()) {
            Some(existing) => {
                if existing.same_payload_as(&incoming) {
                    existing.refresh_last_seen(seen_at);
                    RegistryUpsert::Refreshed(existing.clone())
                } else {
                    let previous = existing.clone();
                    *existing = incoming.clone();
                    let event = AgentEvent::Updated {
                        previous: previous.clone(),
                        current: incoming.clone(),
                        origin,
                    };
                    let _ = self.events_tx.send(event);
                    RegistryUpsert::Updated {
                        previous,
                        current: incoming,
                    }
                }
            }
            None => {
                registry.insert(incoming.id().to_owned(), incoming.clone());
                let _ = self.events_tx.send(AgentEvent::Joined {
                    agent: incoming.clone(),
                    origin,
                });
                RegistryUpsert::Inserted(incoming)
            }
        }
    }

    /// Removes an agent by identifier.
    pub async fn remove(&self, agent_id: &str) -> Option<AgentInfo> {
        self.remove_remote(agent_id).await
    }

    /// Removes the local agent by identifier and emits a local-origin leave event.
    pub async fn remove_local(&self, agent_id: &str) -> Option<AgentInfo> {
        self.remove_with_origin(agent_id, EventOrigin::Local, DepartureReason::Graceful)
            .await
    }

    /// Removes a remote agent by identifier and emits a graceful remote leave event.
    pub async fn remove_remote(&self, agent_id: &str) -> Option<AgentInfo> {
        self.remove_with_origin(agent_id, EventOrigin::Remote, DepartureReason::Graceful)
            .await
    }

    async fn remove_with_origin(
        &self,
        agent_id: &str,
        origin: EventOrigin,
        reason: DepartureReason,
    ) -> Option<AgentInfo> {
        let mut registry = self.inner.write().await;
        let removed = registry.remove(agent_id);
        if let Some(agent) = &removed {
            let _ = self.events_tx.send(AgentEvent::Left {
                agent: agent.clone(),
                origin,
                reason,
            });
        }
        removed
    }

    /// Removes an agent by DNS-SD instance name.
    pub async fn remove_by_instance_name(&self, instance_name: &str) -> Option<AgentInfo> {
        self.remove_remote_by_instance_name(instance_name).await
    }

    /// Removes a remote agent by DNS-SD instance name.
    pub async fn remove_remote_by_instance_name(&self, instance_name: &str) -> Option<AgentInfo> {
        let mut registry = self.inner.write().await;
        let agent_id = registry
            .iter()
            .find(|(_, agent)| agent.instance_name() == instance_name)
            .map(|(agent_id, _)| agent_id.clone())?;

        let removed = registry.remove(&agent_id);
        if let Some(agent) = &removed {
            let _ = self.events_tx.send(AgentEvent::Left {
                agent: agent.clone(),
                origin: EventOrigin::Remote,
                reason: DepartureReason::Graceful,
            });
        }
        removed
    }

    /// Returns a copy of the agent state for the provided identifier.
    pub async fn get(&self, agent_id: &str) -> Option<AgentInfo> {
        let registry = self.inner.read().await;
        registry.get(agent_id).cloned()
    }

    /// Returns all known agents.
    pub async fn list(&self) -> Vec<AgentInfo> {
        let registry = self.inner.read().await;
        let mut agents = registry.values().cloned().collect::<Vec<_>>();
        agents.sort_by(|left, right| left.id().cmp(right.id()));
        agents
    }

    /// Returns all known agents within a project namespace.
    pub async fn get_all_by_project(&self, project: &str) -> Vec<AgentInfo> {
        let registry = self.inner.read().await;
        let mut agents = registry
            .values()
            .filter(|agent| agent.project() == project)
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.id().cmp(right.id()));
        agents
    }

    /// Returns all known agents currently attached to a branch or workstream.
    pub async fn get_all_by_branch(&self, branch: &str) -> Vec<AgentInfo> {
        let registry = self.inner.read().await;
        let mut agents = registry
            .values()
            .filter(|agent| agent.branch() == branch)
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.id().cmp(right.id()));
        agents
    }

    /// Returns all known agents matching both project and branch.
    pub async fn get_all_by_project_and_branch(
        &self,
        project: &str,
        branch: &str,
    ) -> Vec<AgentInfo> {
        let registry = self.inner.read().await;
        let mut agents = registry
            .values()
            .filter(|agent| agent.project() == project && agent.branch() == branch)
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.id().cmp(right.id()));
        agents
    }

    /// Returns all known agents matching a specific status.
    pub async fn get_all_by_status(&self, status: crate::types::AgentStatus) -> Vec<AgentInfo> {
        let registry = self.inner.read().await;
        let mut agents = registry
            .values()
            .filter(|agent| agent.status() == status)
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.id().cmp(right.id()));
        agents
    }

    /// Sweeps the registry and evicts stale peers.
    pub async fn evict_stale(&self) -> Vec<AgentInfo> {
        self.evict_stale_at(Instant::now()).await
    }

    pub(crate) async fn evict_stale_at(&self, now: Instant) -> Vec<AgentInfo> {
        let mut registry = self.inner.write().await;
        let stale_ids = registry
            .iter()
            .filter(|(_, agent)| agent.is_stale(now, self.ttl))
            .map(|(agent_id, _)| agent_id.clone())
            .collect::<Vec<_>>();

        let mut evicted = Vec::with_capacity(stale_ids.len());
        for agent_id in stale_ids {
            if let Some(agent) = registry.remove(&agent_id) {
                let _ = self.events_tx.send(AgentEvent::Left {
                    agent: agent.clone(),
                    origin: EventOrigin::Remote,
                    reason: DepartureReason::Expired,
                });
                evicted.push(agent);
            }
        }

        evicted
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    };

    use super::*;
    use crate::types::{AgentMetadata, AgentStatus};

    fn announcement(agent_id: &str, project: &str, status: AgentStatus) -> AgentAnnouncement {
        let mut metadata = AgentMetadata::new();
        metadata.insert("agent_id".into(), agent_id.into());
        metadata.insert("current_project".into(), project.into());
        metadata.insert("role".into(), "coder".into());
        metadata.insert("status".into(), status.as_str().into());

        AgentAnnouncement::new(
            format!("{agent_id}._agent-mesh._tcp.local."),
            agent_id,
            "coder",
            project,
            "main",
            status,
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            metadata,
        )
        .expect("announcement should be valid")
    }

    #[tokio::test]
    async fn registry_should_insert_and_filter_by_project() {
        let registry = Registry::new(Duration::from_secs(120));
        registry
            .upsert(announcement("agent-a", "alpha", AgentStatus::Idle))
            .await;
        registry
            .upsert(announcement("agent-b", "beta", AgentStatus::Busy))
            .await;

        let alpha = registry.get_all_by_project("alpha").await;
        let all = registry.list().await;

        assert_eq!(alpha.len(), 1);
        assert_eq!(alpha[0].id(), "agent-a");
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn registry_should_filter_by_branch_and_status() {
        let registry = Registry::new(Duration::from_secs(120));
        registry
            .upsert(announcement("agent-a", "alpha", AgentStatus::Idle))
            .await;
        registry
            .upsert(announcement("agent-b", "beta", AgentStatus::Busy))
            .await;

        let main = registry.get_all_by_branch("main").await;
        let alpha_main = registry
            .get_all_by_project_and_branch("alpha", "main")
            .await;
        let busy = registry.get_all_by_status(AgentStatus::Busy).await;

        assert_eq!(main.len(), 2);
        assert_eq!(alpha_main.len(), 1);
        assert_eq!(alpha_main[0].id(), "agent-a");
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0].id(), "agent-b");
    }

    #[tokio::test]
    async fn registry_should_distinguish_insert_update_and_refresh() {
        let registry = Registry::new(Duration::from_secs(120));

        let inserted = registry
            .upsert(announcement("agent-a", "alpha", AgentStatus::Idle))
            .await;
        let refreshed = registry
            .upsert(announcement("agent-a", "alpha", AgentStatus::Idle))
            .await;
        let updated = registry
            .upsert(announcement("agent-a", "alpha", AgentStatus::Busy))
            .await;

        assert!(matches!(inserted, RegistryUpsert::Inserted(_)));
        assert!(matches!(refreshed, RegistryUpsert::Refreshed(_)));
        assert!(matches!(updated, RegistryUpsert::Updated { .. }));
    }

    #[tokio::test]
    async fn registry_should_evict_stale_agents() {
        let ttl = Duration::from_secs(5);
        let registry = Registry::new(ttl);
        let now = Instant::now();

        registry
            .upsert_at(announcement("agent-a", "alpha", AgentStatus::Idle), now)
            .await;
        registry
            .upsert_at(
                announcement("agent-b", "alpha", AgentStatus::Idle),
                now - Duration::from_secs(10),
            )
            .await;

        let evicted = registry.evict_stale_at(now).await;
        let remaining = registry.list().await;

        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].id(), "agent-b");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id(), "agent-a");
    }

    #[tokio::test]
    async fn registry_should_remove_by_instance_name() {
        let registry = Registry::new(Duration::from_secs(120));
        registry
            .upsert(announcement("agent-a", "alpha", AgentStatus::Idle))
            .await;

        let removed = registry
            .remove_by_instance_name("agent-a._agent-mesh._tcp.local.")
            .await;

        assert!(removed.is_some());
        assert!(registry.get("agent-a").await.is_none());
    }

    #[tokio::test]
    async fn registry_should_emit_origin_and_reason_metadata() {
        let registry = Registry::new(Duration::from_secs(120));
        let mut events = registry.subscribe();

        registry
            .upsert_local(announcement("agent-a", "alpha", AgentStatus::Idle))
            .await;
        registry.remove_local("agent-a").await;

        let joined = events.recv().await.expect("joined event should be sent");
        let left = events.recv().await.expect("left event should be sent");

        assert!(matches!(
            joined,
            AgentEvent::Joined {
                origin: EventOrigin::Local,
                ..
            }
        ));
        assert!(matches!(
            left,
            AgentEvent::Left {
                origin: EventOrigin::Local,
                reason: DepartureReason::Graceful,
                ..
            }
        ));
    }
}
