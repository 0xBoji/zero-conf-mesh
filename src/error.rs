use std::time::Duration;

use thiserror::Error;

/// Errors produced by `zero-conf-mesh`.
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
    /// mDNS/DNS-SD service construction failed.
    #[error("mdns error: {0}")]
    Mdns(#[from] mdns_sd::Error),
    /// Background runtime task failed to join.
    #[error("background task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}
