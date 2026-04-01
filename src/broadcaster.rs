use mdns_sd::{ServiceDaemon, UnregisterStatus};
use tracing::debug;

use crate::{error::ZeroConfError, types::AgentAnnouncement};

/// Thin wrapper around `mdns-sd` registration APIs for the local node.
#[derive(Clone)]
pub(crate) struct Broadcaster {
    daemon: ServiceDaemon,
    service_type: String,
    host_name: String,
}

impl Broadcaster {
    pub(crate) fn new(
        daemon: ServiceDaemon,
        service_type: impl Into<String>,
        host_name: impl Into<String>,
    ) -> Self {
        Self {
            daemon,
            service_type: service_type.into(),
            host_name: host_name.into(),
        }
    }

    pub(crate) fn announce(&self, announcement: &AgentAnnouncement) -> Result<(), ZeroConfError> {
        let service = announcement
            .to_service_info(&self.service_type, &self.host_name)?
            .enable_addr_auto();
        self.daemon.register(service)?;
        debug!(
            instance = announcement.instance_name(),
            "registered local service"
        );
        Ok(())
    }

    pub(crate) async fn unregister(
        &self,
        announcement: &AgentAnnouncement,
    ) -> Result<(), ZeroConfError> {
        let receiver = self.daemon.unregister(announcement.instance_name())?;
        match receiver.recv_async().await {
            Ok(UnregisterStatus::OK | UnregisterStatus::NotFound) | Err(_) => Ok(()),
        }
    }
}
