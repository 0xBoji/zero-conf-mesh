use std::{collections::BTreeMap, fmt, net::IpAddr, str::FromStr, time::Instant};

use mdns_sd::{ResolvedService, ServiceInfo, TxtProperties, TxtProperty};
use serde::{Deserialize, Serialize};

use crate::error::ZeroConfError;

/// Metadata key used for the advertised agent identifier.
pub const AGENT_ID_METADATA_KEY: &str = "agent_id";
/// Metadata key used for the advertised agent role.
pub const AGENT_ROLE_METADATA_KEY: &str = "role";
/// Metadata key used for the advertised project name.
pub const AGENT_PROJECT_METADATA_KEY: &str = "current_project";
/// Metadata key used for the advertised git/work branch name.
pub const AGENT_BRANCH_METADATA_KEY: &str = "current_branch";
/// Metadata key used for the advertised agent status.
pub const AGENT_STATUS_METADATA_KEY: &str = "status";

/// Additional metadata attached to an agent advertisement.
pub type AgentMetadata = BTreeMap<String, String>;

/// Typed operational status for an agent in the mesh.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// The agent is ready to accept new work.
    #[default]
    Idle,
    /// The agent is actively working.
    Busy,
    /// The agent is in a degraded or failed state.
    Error,
}

impl AgentStatus {
    /// Returns the canonical wire-format string for this status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentStatus {
    type Err = ZeroConfError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "idle" => Ok(Self::Idle),
            "busy" => Ok(Self::Busy),
            "error" => Ok(Self::Error),
            _ => Err(ZeroConfError::InvalidStatus { value: normalized }),
        }
    }
}

/// Serializable data received from or prepared for an mDNS/DNS-SD announcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAnnouncement {
    instance_name: String,
    agent_id: String,
    role: String,
    project: String,
    branch: String,
    status: AgentStatus,
    port: u16,
    addresses: Vec<IpAddr>,
    metadata: AgentMetadata,
}

impl AgentAnnouncement {
    /// Creates a validated agent announcement payload.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when required fields are empty or the port is zero.
    #[expect(
        clippy::too_many_arguments,
        reason = "announcement fields are explicit to keep wire payload construction readable"
    )]
    pub fn new(
        instance_name: impl Into<String>,
        agent_id: impl Into<String>,
        role: impl Into<String>,
        project: impl Into<String>,
        branch: impl Into<String>,
        status: AgentStatus,
        port: u16,
        addresses: Vec<IpAddr>,
        metadata: AgentMetadata,
    ) -> Result<Self, ZeroConfError> {
        let instance_name = normalize_required(instance_name.into(), "instance_name")?;
        let agent_id = normalize_required(agent_id.into(), "agent_id")?;
        let role = normalize_required(role.into(), "role")?;
        let project = normalize_required(project.into(), "project")?;
        let branch = normalize_required(branch.into(), "branch")?;

        if port == 0 {
            return Err(ZeroConfError::InvalidPort);
        }

        if metadata.keys().any(|key| key.trim().is_empty()) {
            return Err(ZeroConfError::EmptyMetadataKey);
        }

        Ok(Self {
            instance_name,
            agent_id,
            role,
            project,
            branch,
            status,
            port,
            addresses,
            metadata,
        })
    }

    /// Returns the DNS-SD instance name.
    #[must_use]
    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    /// Returns the unique agent identifier.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Returns the declared agent role.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the declared project namespace.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Returns the declared branch or workstream identifier.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the current operational status.
    #[must_use]
    pub const fn status(&self) -> AgentStatus {
        self.status
    }

    /// Returns the advertised service port.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Returns the known IP addresses for the agent.
    #[must_use]
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    /// Returns the announcement metadata.
    #[must_use]
    pub const fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// Updates the current status and synchronizes the canonical status metadata key.
    pub fn set_status(&mut self, status: AgentStatus) {
        self.status = status;
        self.metadata.insert(
            AGENT_STATUS_METADATA_KEY.to_owned(),
            status.as_str().to_owned(),
        );
    }

    /// Updates the current project namespace and synchronizes the canonical TXT key.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the provided project is empty after trimming.
    pub fn set_project(&mut self, project: impl Into<String>) -> Result<(), ZeroConfError> {
        let project = normalize_required(project.into(), "project")?;
        self.metadata
            .insert(AGENT_PROJECT_METADATA_KEY.to_owned(), project.clone());
        self.project = project;
        Ok(())
    }

    /// Updates the current branch/workstream and synchronizes the canonical TXT key.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the provided branch is empty after trimming.
    pub fn set_branch(&mut self, branch: impl Into<String>) -> Result<(), ZeroConfError> {
        let branch = normalize_required(branch.into(), "branch")?;
        self.metadata
            .insert(AGENT_BRANCH_METADATA_KEY.to_owned(), branch.clone());
        self.branch = branch;
        Ok(())
    }

    /// Updates a non-canonical metadata entry.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the key is empty or reserved by the crate.
    pub fn set_metadata(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), ZeroConfError> {
        let key = normalize_metadata_key(key.into())?;
        if is_canonical_metadata_key(&key) {
            return Err(ZeroConfError::ReservedMetadataKey { key });
        }

        self.metadata.insert(key, value.into());
        Ok(())
    }

    /// Converts this announcement into `mdns-sd` TXT properties.
    #[must_use]
    pub fn to_txt_properties(&self) -> Vec<TxtProperty> {
        let mut metadata = self.metadata.clone();
        metadata.insert(AGENT_ID_METADATA_KEY.to_owned(), self.agent_id.clone());
        metadata.insert(AGENT_ROLE_METADATA_KEY.to_owned(), self.role.clone());
        metadata.insert(AGENT_PROJECT_METADATA_KEY.to_owned(), self.project.clone());
        metadata.insert(AGENT_BRANCH_METADATA_KEY.to_owned(), self.branch.clone());
        metadata.insert(
            AGENT_STATUS_METADATA_KEY.to_owned(),
            self.status.as_str().to_owned(),
        );

        metadata.into_iter().map(TxtProperty::from).collect()
    }

    /// Converts this announcement into an mDNS/DNS-SD service descriptor.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when the host name or service payload is invalid.
    pub fn to_service_info(
        &self,
        service_type: &str,
        host_name: &str,
    ) -> Result<ServiceInfo, ZeroConfError> {
        ServiceInfo::new(
            service_type,
            &self.agent_id,
            host_name,
            self.addresses.as_slice(),
            self.port,
            self.to_txt_properties(),
        )
        .map_err(ZeroConfError::from)
    }

    /// Creates an announcement from a resolved mDNS/DNS-SD service.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when required TXT properties are missing or invalid.
    pub fn from_resolved_service(service: &ResolvedService) -> Result<Self, ZeroConfError> {
        let addresses = service
            .get_addresses()
            .iter()
            .map(mdns_sd::ScopedIp::to_ip_addr)
            .collect();

        Self::from_txt_properties(
            service.get_fullname(),
            service.get_port(),
            addresses,
            service.get_properties(),
        )
    }

    /// Creates an announcement from TXT properties collected from the network.
    ///
    /// # Errors
    /// Returns [`ZeroConfError`] when required TXT properties are missing or invalid.
    pub fn from_txt_properties(
        instance_name: impl Into<String>,
        port: u16,
        addresses: Vec<IpAddr>,
        properties: &TxtProperties,
    ) -> Result<Self, ZeroConfError> {
        let metadata = metadata_from_txt_properties(properties)?;
        let agent_id = required_metadata(&metadata, AGENT_ID_METADATA_KEY)?.to_owned();
        let role = required_metadata(&metadata, AGENT_ROLE_METADATA_KEY)?.to_owned();
        let project = required_metadata(&metadata, AGENT_PROJECT_METADATA_KEY)?.to_owned();
        let branch = required_metadata(&metadata, AGENT_BRANCH_METADATA_KEY)?.to_owned();
        let status = required_metadata(&metadata, AGENT_STATUS_METADATA_KEY)?.parse()?;

        Self::new(
            instance_name,
            agent_id,
            role,
            project,
            branch,
            status,
            port,
            addresses,
            metadata,
        )
    }

    pub(crate) fn into_agent_info(self, seen_at: Instant) -> AgentInfo {
        AgentInfo {
            instance_name: self.instance_name,
            id: self.agent_id,
            role: self.role,
            project: self.project,
            branch: self.branch,
            status: self.status,
            port: self.port,
            addresses: self.addresses,
            metadata: self.metadata,
            last_seen: seen_at,
        }
    }
}

/// Runtime representation of a discovered agent in the local registry.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    instance_name: String,
    id: String,
    role: String,
    project: String,
    branch: String,
    status: AgentStatus,
    port: u16,
    addresses: Vec<IpAddr>,
    metadata: AgentMetadata,
    last_seen: Instant,
}

impl AgentInfo {
    /// Returns the DNS-SD instance name.
    #[must_use]
    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    /// Returns the unique agent identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the agent role.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the project namespace.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Returns the branch or workstream identifier.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the current operational status.
    #[must_use]
    pub const fn status(&self) -> AgentStatus {
        self.status
    }

    /// Returns the advertised service port.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Returns the known IP addresses for this agent.
    #[must_use]
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    /// Returns the metadata persisted for this agent.
    #[must_use]
    pub const fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// Returns the monotonic timestamp of the last successful observation.
    #[must_use]
    pub const fn last_seen(&self) -> Instant {
        self.last_seen
    }

    pub(crate) fn same_payload_as(&self, other: &Self) -> bool {
        self.instance_name == other.instance_name
            && self.id == other.id
            && self.role == other.role
            && self.project == other.project
            && self.branch == other.branch
            && self.status == other.status
            && self.port == other.port
            && self.addresses == other.addresses
            && self.metadata == other.metadata
    }

    pub(crate) fn refresh_last_seen(&mut self, seen_at: Instant) {
        self.last_seen = seen_at;
    }

    pub(crate) fn is_stale(&self, now: Instant, ttl: std::time::Duration) -> bool {
        now.saturating_duration_since(self.last_seen) > ttl
    }
}

fn normalize_required(value: String, field: &'static str) -> Result<String, ZeroConfError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ZeroConfError::EmptyField { field });
    }
    Ok(trimmed.to_owned())
}

fn normalize_metadata_key(key: String) -> Result<String, ZeroConfError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(ZeroConfError::EmptyMetadataKey);
    }

    Ok(trimmed.to_owned())
}

fn is_canonical_metadata_key(key: &str) -> bool {
    matches!(
        key,
        AGENT_ID_METADATA_KEY
            | AGENT_ROLE_METADATA_KEY
            | AGENT_PROJECT_METADATA_KEY
            | AGENT_BRANCH_METADATA_KEY
            | AGENT_STATUS_METADATA_KEY
    )
}

fn metadata_from_txt_properties(
    properties: &TxtProperties,
) -> Result<AgentMetadata, ZeroConfError> {
    properties
        .iter()
        .map(|property| {
            let value = match property.val() {
                Some(value) => String::from_utf8(value.to_vec()).map_err(|_| {
                    ZeroConfError::InvalidTxtPropertyEncoding {
                        key: property.key().to_owned(),
                    }
                })?,
                None => String::new(),
            };

            Ok((property.key().to_owned(), value))
        })
        .collect()
}

fn required_metadata<'a>(
    metadata: &'a AgentMetadata,
    key: &'static str,
) -> Result<&'a str, ZeroConfError> {
    metadata
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ZeroConfError::MissingTxtProperty { key })
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use mdns_sd::IntoTxtProperties;

    use super::*;

    fn announcement() -> AgentAnnouncement {
        let mut metadata = AgentMetadata::new();
        metadata.insert("capability".into(), "review".into());

        AgentAnnouncement::new(
            "agent-1._agent-mesh._tcp.local.",
            "agent-1",
            "reviewer",
            "alpha",
            "main",
            AgentStatus::Busy,
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            metadata,
        )
        .expect("announcement should be valid")
    }

    #[test]
    fn agent_announcement_should_convert_to_service_info() {
        let announcement = announcement();

        let service = announcement
            .to_service_info("_agent-mesh._tcp.local.", "agent-1.local.")
            .expect("service info should be created");

        assert_eq!(service.get_fullname(), "agent-1._agent-mesh._tcp.local.");
        assert_eq!(service.get_port(), 8080);
        assert_eq!(
            service.get_property_val_str(AGENT_ID_METADATA_KEY),
            Some("agent-1")
        );
        assert_eq!(
            service.get_property_val_str(AGENT_STATUS_METADATA_KEY),
            Some("busy")
        );
        assert_eq!(service.get_property_val_str("capability"), Some("review"));
    }

    #[test]
    fn agent_announcement_should_round_trip_from_resolved_service() {
        let properties = [
            ("agent_id", "agent-1"),
            ("role", "reviewer"),
            ("current_project", "alpha"),
            ("current_branch", "main"),
            ("status", "busy"),
            ("capability", "review"),
        ]
        .as_slice()
        .into_txt_properties();

        let announcement = AgentAnnouncement::from_txt_properties(
            "agent-1._agent-mesh._tcp.local.",
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            &properties,
        )
        .expect("TXT properties should parse");

        assert_eq!(announcement.agent_id(), "agent-1");
        assert_eq!(announcement.role(), "reviewer");
        assert_eq!(announcement.project(), "alpha");
        assert_eq!(announcement.branch(), "main");
        assert_eq!(announcement.status(), AgentStatus::Busy);
        assert_eq!(
            announcement.metadata().get("capability"),
            Some(&"review".to_owned())
        );
    }

    #[test]
    fn agent_announcement_should_reject_missing_required_txt_property() {
        let properties = [
            ("agent_id", "agent-1"),
            ("role", "reviewer"),
            ("current_branch", "main"),
            ("status", "busy"),
        ]
        .as_slice()
        .into_txt_properties();

        let err = AgentAnnouncement::from_txt_properties(
            "agent-1._agent-mesh._tcp.local.",
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            &properties,
        )
        .expect_err("missing current_project should fail");

        assert!(matches!(
            err,
            ZeroConfError::MissingTxtProperty {
                key: AGENT_PROJECT_METADATA_KEY
            }
        ));
    }

    #[test]
    fn agent_announcement_should_reject_invalid_status_txt_property() {
        let properties = [
            ("agent_id", "agent-1"),
            ("role", "reviewer"),
            ("current_project", "alpha"),
            ("current_branch", "main"),
            ("status", "offline"),
        ]
        .as_slice()
        .into_txt_properties();

        let err = AgentAnnouncement::from_txt_properties(
            "agent-1._agent-mesh._tcp.local.",
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            &properties,
        )
        .expect_err("invalid status should fail");

        assert!(matches!(err, ZeroConfError::InvalidStatus { .. }));
    }

    #[test]
    fn agent_announcement_should_reject_invalid_utf8_txt_property() {
        let properties = vec![
            TxtProperty::from(("agent_id", "agent-1")),
            TxtProperty::from(("role", "reviewer")),
            TxtProperty::from(("current_project", "alpha")),
            TxtProperty::from(("current_branch", "main")),
            TxtProperty::from(("status", "busy")),
            TxtProperty::from(("capability", vec![0xff, 0xfe, 0xfd])),
        ]
        .into_txt_properties();

        let err = AgentAnnouncement::from_txt_properties(
            "agent-1._agent-mesh._tcp.local.",
            8080,
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            &properties,
        )
        .expect_err("invalid utf8 metadata should fail");

        assert!(matches!(
            err,
            ZeroConfError::InvalidTxtPropertyEncoding { key }
            if key == "capability"
        ));
    }

    #[test]
    fn agent_announcement_should_update_project_branch_and_metadata() {
        let mut announcement = announcement();

        announcement
            .set_project("beta")
            .expect("project update should succeed");
        announcement
            .set_branch("feature/runtime")
            .expect("branch update should succeed");
        announcement
            .set_metadata("capability", "planning")
            .expect("metadata update should succeed");

        assert_eq!(announcement.project(), "beta");
        assert_eq!(announcement.branch(), "feature/runtime");
        assert_eq!(
            announcement.metadata().get(AGENT_PROJECT_METADATA_KEY),
            Some(&"beta".to_owned())
        );
        assert_eq!(
            announcement.metadata().get(AGENT_BRANCH_METADATA_KEY),
            Some(&"feature/runtime".to_owned())
        );
        assert_eq!(
            announcement.metadata().get("capability"),
            Some(&"planning".to_owned())
        );
    }

    #[test]
    fn agent_announcement_should_reject_reserved_metadata_keys() {
        let mut announcement = announcement();

        let err = announcement
            .set_metadata(AGENT_STATUS_METADATA_KEY, "busy")
            .expect_err("canonical metadata keys should be rejected");

        assert!(matches!(err, ZeroConfError::ReservedMetadataKey { key } if key == "status"));
    }
}
