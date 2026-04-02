use std::time::Duration;

use thiserror::Error;

/// Errors produced by `coding_agent_mesh_presence`.
#[derive(Debug, Error)]
pub enum ZeroConfError {
    /// A required text field was empty after trimming whitespace.
    #[error("the `{field}` field must not be empty")]
    EmptyField {
        /// The invalid field name.
        field: &'static str,
    },
    /// The configured service type is not valid for DNS-SD.
    #[error(
        "invalid service type `{service_type}`; expected a `_name._tcp.local.` or `_name._udp.local.` pattern"
    )]
    InvalidServiceType {
        /// The invalid service type.
        service_type: String,
    },
    /// Port zero is not a valid advertised service port.
    #[error("port must be greater than zero")]
    InvalidPort,
    /// Port zero is not a valid mDNS daemon UDP port.
    #[error("mDNS port must be greater than zero")]
    InvalidMdnsPort,
    /// Broadcast event channel capacity must be greater than zero.
    #[error("event capacity must be greater than zero")]
    InvalidEventCapacity,
    /// TTL must be strictly greater than the heartbeat interval.
    #[error("ttl ({ttl:?}) must be greater than heartbeat interval ({heartbeat_interval:?})")]
    InvalidTiming {
        /// The configured heartbeat interval.
        heartbeat_interval: Duration,
        /// The configured TTL.
        ttl: Duration,
    },
    /// Metadata contained an empty key.
    #[error("metadata keys must not be empty")]
    EmptyMetadataKey,
    /// Metadata attempted to overwrite a canonical key managed by the crate.
    #[error("metadata key `{key}` is reserved; use the dedicated runtime updater instead")]
    ReservedMetadataKey {
        /// The reserved metadata key.
        key: String,
    },
    /// Capability names must not be empty after trimming.
    #[error("capability names must not be empty")]
    EmptyCapability,
    /// Shared secrets must not be empty after trimming.
    #[error("shared secret must not be empty")]
    EmptySharedSecret,
    /// Capability names must not contain commas because the wire format uses comma separation.
    #[error("invalid capability `{value}`; capability names must not contain commas")]
    InvalidCapability {
        /// The invalid capability value.
        value: String,
    },
    /// A required TXT property was missing from a discovered service.
    #[error("missing required TXT property `{key}`")]
    MissingTxtProperty {
        /// The missing TXT property key.
        key: &'static str,
    },
    /// A TXT property value was not valid UTF-8.
    #[error("TXT property `{key}` must contain valid UTF-8 data")]
    InvalidTxtPropertyEncoding {
        /// The invalid TXT property key.
        key: String,
    },
    /// A status string could not be parsed into a known enum value.
    #[error("invalid status `{value}`; expected one of: idle, busy, error")]
    InvalidStatus {
        /// The invalid status string.
        value: String,
    },
    /// A metadata regex query could not be compiled.
    #[error("invalid metadata regex `{pattern}`: {source}")]
    InvalidMetadataRegex {
        /// The invalid regex pattern.
        pattern: String,
        /// The underlying regex compilation error.
        #[source]
        source: regex::Error,
    },
    /// Required authentication metadata was not present.
    #[error("missing authentication metadata `{key}`")]
    MissingAuthMetadata {
        /// The missing authentication key.
        key: &'static str,
    },
    /// A discovered peer advertised an unknown authentication scheme.
    #[error("invalid authentication scheme `{value}`; expected `shared_secret_hmac_sha256`")]
    InvalidAuthScheme {
        /// The unexpected auth scheme value.
        value: String,
    },
    /// Shared-secret verification failed for a discovered announcement.
    #[error("shared-secret verification failed for agent `{agent_id}`")]
    InvalidSharedSecretSignature {
        /// The agent identifier associated with the invalid signature.
        agent_id: String,
    },
    /// mDNS/DNS-SD service construction failed.
    #[error("mdns error: {0}")]
    Mdns(#[from] mdns_sd::Error),
    /// Background runtime task failed to join.
    #[error("background task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}
