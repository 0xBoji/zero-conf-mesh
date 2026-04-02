//! `camp` is the command-line interface for `coding_agent_mesh_presence`.
//!
//! It is designed to be friendly to shell-driven and LLM-driven agent workflows:
//! large banner on stderr, structured JSON on stdout.

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::Write,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event as SseEvent, KeepAlive, Sse},
    },
    routing::get,
};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use coding_agent_mesh_presence::{
    AgentEvent, AgentInfo, AgentStatus, DepartureReason, EventOrigin, NetworkInterface,
    SharedSecretMode, ZeroConfMesh,
};
use serde::{Deserialize, Serialize};
use tokio::time;

const DEFAULT_CONFIG_FILE_NAME: &str = ".camp.toml";
const DEFAULT_AGENT_ROLE: &str = "agent";
const DEFAULT_AGENT_PROJECT: &str = "default";
const DEFAULT_AGENT_BRANCH: &str = "main";
const DEFAULT_AGENT_PORT: u16 = 7000;
const DEFAULT_HEARTBEAT_MS: u64 = 30_000;
const DEFAULT_TTL_MS: u64 = 120_000;
const AGENTS_GUIDANCE_START: &str = "<!-- CAMP:START -->";
const AGENTS_GUIDANCE_END: &str = "<!-- CAMP:END -->";

#[derive(Parser, Debug)]
#[command(name = "camp", version, about = "Coding agent mesh presence CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a local camp config and inject project usage guidance into AGENTS.md.
    Init(InitCommand),
    /// Announce the local agent described by the camp config file.
    Up(UpCommand),
    /// Announce a local agent on the LAN and keep it online until interrupted.
    Announce(AnnounceCommand),
    /// Discover peers on the LAN and print the current registry as JSON.
    #[command(alias = "who")]
    List(ListCommand),
    /// Discover a single peer by id and print it as JSON.
    Get(GetCommand),
    /// Watch discovery events and print newline-delimited JSON.
    Watch(WatchCommand),
    /// Start a local REST bridge for non-shell agent frameworks.
    Serve(ServeCommand),
    /// Generate shell completion scripts for camp.
    Completions(CompletionsCommand),
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SharedSecretModeArg {
    SignOnly,
    #[default]
    SignAndVerify,
}

impl From<SharedSecretModeArg> for SharedSecretMode {
    fn from(value: SharedSecretModeArg) -> Self {
        match value {
            SharedSecretModeArg::SignOnly => Self::SignOnly,
            SharedSecretModeArg::SignAndVerify => Self::SignAndVerify,
        }
    }
}

#[derive(Args, Debug, Clone)]
struct DiscoveryOptions {
    /// Read discovery defaults from a camp TOML config file.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Service type to browse.
    #[arg(long, default_value = coding_agent_mesh_presence::DEFAULT_SERVICE_TYPE)]
    service_type: String,
    /// UDP port used by the embedded mDNS daemon.
    #[arg(long, default_value_t = coding_agent_mesh_presence::DEFAULT_MDNS_PORT)]
    mdns_port: u16,
    /// Milliseconds to wait for discovery before reading the registry.
    #[arg(long, default_value_t = 1_500)]
    discover_ms: u64,
    /// Shared secret used for signing / verification.
    #[arg(long)]
    shared_secret: Option<String>,
    /// Additional accepted secrets during rotation.
    #[arg(long = "shared-secret-accept")]
    shared_secret_accept: Vec<String>,
    /// Shared-secret mode.
    #[arg(long, value_enum, default_value_t = SharedSecretModeArg::SignAndVerify)]
    shared_secret_mode: SharedSecretModeArg,
    /// Include only matching interfaces for the embedded mDNS daemon.
    #[arg(long = "enable-interface")]
    enable_interface: Vec<String>,
    /// Exclude matching interfaces for the embedded mDNS daemon.
    #[arg(long = "disable-interface")]
    disable_interface: Vec<String>,
}

#[derive(Args, Debug)]
struct InitCommand {
    /// Output camp config path.
    #[arg(long, default_value = DEFAULT_CONFIG_FILE_NAME)]
    config: PathBuf,
    /// AGENTS.md file that should receive camp usage guidance.
    #[arg(long, default_value = "AGENTS.md")]
    agents_file: PathBuf,
    /// Overwrite an existing camp config file.
    #[arg(long)]
    force: bool,
    /// Local agent id. Defaults to a host/user-derived slug.
    #[arg(long)]
    id: Option<String>,
    /// Local role.
    #[arg(long)]
    role: Option<String>,
    /// Project namespace.
    #[arg(long)]
    project: Option<String>,
    /// Branch/workstream identifier.
    #[arg(long)]
    branch: Option<String>,
    /// Advertised TCP service port.
    #[arg(long)]
    port: Option<u16>,
    /// Initial agent status.
    #[arg(long, default_value = "idle", value_parser = parse_status)]
    status: AgentStatus,
    /// Additional typed capability.
    #[arg(long = "capability")]
    capabilities: Vec<String>,
    /// Extra metadata entry in KEY=VALUE form.
    #[arg(long = "metadata", value_parser = parse_key_value)]
    metadata: Vec<(String, String)>,
    /// Service type to announce.
    #[arg(long, default_value = coding_agent_mesh_presence::DEFAULT_SERVICE_TYPE)]
    service_type: String,
    /// UDP port used by the embedded mDNS daemon.
    #[arg(long, default_value_t = coding_agent_mesh_presence::DEFAULT_MDNS_PORT)]
    mdns_port: u16,
    /// Heartbeat interval in milliseconds.
    #[arg(long, default_value_t = DEFAULT_HEARTBEAT_MS)]
    heartbeat_ms: u64,
    /// TTL in milliseconds.
    #[arg(long, default_value_t = DEFAULT_TTL_MS)]
    ttl_ms: u64,
    /// Shared secret used for signing / verification.
    #[arg(long)]
    shared_secret: Option<String>,
    /// Additional accepted secrets during rotation.
    #[arg(long = "shared-secret-accept")]
    shared_secret_accept: Vec<String>,
    /// Shared-secret mode.
    #[arg(long, value_enum, default_value_t = SharedSecretModeArg::SignAndVerify)]
    shared_secret_mode: SharedSecretModeArg,
    /// Include only matching interfaces for the embedded mDNS daemon.
    #[arg(long = "enable-interface")]
    enable_interface: Vec<String>,
    /// Exclude matching interfaces for the embedded mDNS daemon.
    #[arg(long = "disable-interface")]
    disable_interface: Vec<String>,
}

#[derive(Args, Debug)]
struct UpCommand {
    /// camp TOML config to announce from.
    #[arg(long, default_value = DEFAULT_CONFIG_FILE_NAME)]
    config: PathBuf,
    /// Optional maximum lifetime in seconds; otherwise waits for Ctrl-C.
    #[arg(long)]
    duration_secs: Option<u64>,
    /// Print the local announcement as JSON once startup completes.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct AnnounceCommand {
    /// Local agent id.
    #[arg(long)]
    id: String,
    /// Local role.
    #[arg(long, default_value = "agent")]
    role: String,
    /// Project namespace.
    #[arg(long, default_value = "default")]
    project: String,
    /// Branch/workstream identifier.
    #[arg(long, default_value = "unknown")]
    branch: String,
    /// Advertised TCP service port.
    #[arg(long)]
    port: u16,
    /// Initial agent status.
    #[arg(long, default_value = "idle", value_parser = parse_status)]
    status: AgentStatus,
    /// Additional typed capability.
    #[arg(long = "capability")]
    capabilities: Vec<String>,
    /// Extra metadata entry in KEY=VALUE form.
    #[arg(long = "metadata", value_parser = parse_key_value)]
    metadata: Vec<(String, String)>,
    /// Service type to announce.
    #[arg(long, default_value = coding_agent_mesh_presence::DEFAULT_SERVICE_TYPE)]
    service_type: String,
    /// UDP port used by the embedded mDNS daemon.
    #[arg(long, default_value_t = coding_agent_mesh_presence::DEFAULT_MDNS_PORT)]
    mdns_port: u16,
    /// Heartbeat interval in milliseconds.
    #[arg(long, default_value_t = 30_000)]
    heartbeat_ms: u64,
    /// TTL in milliseconds.
    #[arg(long, default_value_t = 120_000)]
    ttl_ms: u64,
    /// Shared secret used for signing / verification.
    #[arg(long)]
    shared_secret: Option<String>,
    /// Additional accepted secrets during rotation.
    #[arg(long = "shared-secret-accept")]
    shared_secret_accept: Vec<String>,
    /// Shared-secret mode.
    #[arg(long, value_enum, default_value_t = SharedSecretModeArg::SignAndVerify)]
    shared_secret_mode: SharedSecretModeArg,
    /// Include only matching interfaces for the embedded mDNS daemon.
    #[arg(long = "enable-interface")]
    enable_interface: Vec<String>,
    /// Exclude matching interfaces for the embedded mDNS daemon.
    #[arg(long = "disable-interface")]
    disable_interface: Vec<String>,
    /// Optional maximum lifetime in seconds; otherwise waits for Ctrl-C.
    #[arg(long)]
    duration_secs: Option<u64>,
    /// Print the local announcement as JSON once startup completes.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ListCommand {
    #[command(flatten)]
    discovery: DiscoveryOptions,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    role: Option<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long, value_parser = parse_status)]
    status: Option<AgentStatus>,
    #[arg(long)]
    capability: Option<String>,
    #[arg(long = "metadata", value_parser = parse_key_value)]
    metadata: Vec<(String, String)>,
    #[arg(long = "metadata-key")]
    metadata_keys: Vec<String>,
    #[arg(long = "metadata-key-prefix")]
    metadata_key_prefixes: Vec<String>,
    #[arg(long = "metadata-prefix", value_parser = parse_key_value)]
    metadata_prefixes: Vec<(String, String)>,
    #[arg(long = "metadata-regex", value_parser = parse_key_value)]
    metadata_regexes: Vec<(String, String)>,
}

#[derive(Args, Debug)]
struct GetCommand {
    #[command(flatten)]
    discovery: DiscoveryOptions,
    /// Agent id to fetch.
    id: String,
}

#[derive(Args, Debug)]
struct WatchCommand {
    #[command(flatten)]
    discovery: DiscoveryOptions,
    /// Write the full discovered registry state to a JSON file on every change.
    #[arg(long)]
    write_state: Option<PathBuf>,
    /// Append newline-delimited JSON events to a log file.
    #[arg(long)]
    write_events: Option<PathBuf>,
    /// Execute a shell command for each snapshot/event and send JSON to stdin.
    #[arg(long)]
    exec: Option<String>,
}

#[derive(Args, Debug)]
struct ServeCommand {
    #[command(flatten)]
    discovery: DiscoveryOptions,
    /// Bind address for the local HTTP bridge.
    #[arg(long, default_value = "127.0.0.1:9999")]
    bind: String,
}

#[derive(Args, Debug)]
struct CompletionsCommand {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    shell: Shell,
}

#[derive(Debug, Serialize)]
struct AgentRecord {
    id: String,
    instance_name: String,
    role: String,
    project: String,
    branch: String,
    status: String,
    capabilities: Vec<String>,
    port: u16,
    addresses: Vec<String>,
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct EventRecord {
    kind: &'static str,
    origin: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<AgentRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<AgentRecord>,
}

#[derive(Debug, Serialize)]
struct SnapshotRecord {
    kind: &'static str,
    agents: Vec<AgentRecord>,
}

#[derive(Debug, Serialize)]
struct HealthRecord {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct AgentQuery {
    id: Option<String>,
    role: Option<String>,
    project: Option<String>,
    branch: Option<String>,
    status: Option<String>,
    capability: Option<String>,
}

#[derive(Clone)]
struct ServeState {
    mesh: Arc<ZeroConfMesh>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CampConfigFile {
    agent: CampAgentConfig,
    discovery: CampDiscoveryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CampAgentConfig {
    id: String,
    role: String,
    project: String,
    branch: String,
    port: u16,
    status: AgentStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CampDiscoveryConfig {
    service_type: String,
    mdns_port: u16,
    heartbeat_ms: u64,
    ttl_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shared_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shared_secret_accept: Vec<String>,
    #[serde(default)]
    shared_secret_mode: SharedSecretModeArg,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    enable_interface: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disable_interface: Vec<String>,
}

#[derive(Debug, Clone)]
struct AnnounceSettings {
    id: String,
    role: String,
    project: String,
    branch: String,
    port: u16,
    status: AgentStatus,
    capabilities: Vec<String>,
    metadata: Vec<(String, String)>,
    service_type: String,
    mdns_port: u16,
    heartbeat_ms: u64,
    ttl_ms: u64,
    shared_secret: Option<String>,
    shared_secret_accept: Vec<String>,
    shared_secret_mode: SharedSecretMode,
    enable_interface: Vec<String>,
    disable_interface: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedDiscoveryOptions {
    service_type: String,
    mdns_port: u16,
    shared_secret: Option<String>,
    shared_secret_accept: Vec<String>,
    shared_secret_mode: SharedSecretMode,
    enable_interface: Vec<String>,
    disable_interface: Vec<String>,
}

impl From<CampConfigFile> for AnnounceSettings {
    fn from(value: CampConfigFile) -> Self {
        let CampConfigFile { agent, discovery } = value;
        Self {
            id: agent.id,
            role: agent.role,
            project: agent.project,
            branch: agent.branch,
            port: agent.port,
            status: agent.status,
            capabilities: agent.capabilities,
            metadata: agent.metadata.into_iter().collect(),
            service_type: discovery.service_type,
            mdns_port: discovery.mdns_port,
            heartbeat_ms: discovery.heartbeat_ms,
            ttl_ms: discovery.ttl_ms,
            shared_secret: discovery.shared_secret,
            shared_secret_accept: discovery.shared_secret_accept,
            shared_secret_mode: discovery.shared_secret_mode.into(),
            enable_interface: discovery.enable_interface,
            disable_interface: discovery.disable_interface,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    print_banner();
    let cli = Cli::parse();

    match cli.command {
        Command::Init(command) => run_init(command)?,
        Command::Up(command) => run_up(command).await?,
        Command::Announce(command) => run_announce(command).await?,
        Command::List(command) => run_list(command).await?,
        Command::Get(command) => run_get(command).await?,
        Command::Watch(command) => run_watch(command).await?,
        Command::Serve(command) => run_serve(command).await?,
        Command::Completions(command) => run_completions(command)?,
    }

    Ok(())
}

fn run_init(command: InitCommand) -> Result<(), Box<dyn Error>> {
    let config_path = command.config;
    if config_path.exists() && !command.force {
        return Err(format!(
            "camp config already exists at {}; rerun with --force to overwrite",
            config_path.display()
        )
        .into());
    }

    let project = command.project.unwrap_or_else(infer_default_project);
    let branch = command.branch.unwrap_or_else(infer_default_branch);
    let agent_id = command
        .id
        .unwrap_or_else(|| infer_default_agent_id(&project, &branch));

    let config = CampConfigFile {
        agent: CampAgentConfig {
            id: agent_id,
            role: command
                .role
                .unwrap_or_else(|| DEFAULT_AGENT_ROLE.to_owned()),
            project,
            branch,
            port: command.port.unwrap_or(DEFAULT_AGENT_PORT),
            status: command.status,
            capabilities: command.capabilities,
            metadata: command.metadata.into_iter().collect(),
        },
        discovery: CampDiscoveryConfig {
            service_type: command.service_type,
            mdns_port: command.mdns_port,
            heartbeat_ms: command.heartbeat_ms,
            ttl_ms: command.ttl_ms,
            shared_secret: command.shared_secret,
            shared_secret_accept: command.shared_secret_accept,
            shared_secret_mode: command.shared_secret_mode,
            enable_interface: command.enable_interface,
            disable_interface: command.disable_interface,
        },
    };

    write_camp_config(&config_path, &config)?;
    upsert_agents_guidance(&command.agents_file, &config_path, &config)?;

    eprintln!("camp: wrote {}", config_path.display());
    eprintln!("camp: updated {}", command.agents_file.display());
    eprintln!("camp: next steps");
    eprintln!("  1. camp up");
    eprintln!(
        "  2. camp who --config {} --project {}",
        config_path.display(),
        config.agent.project
    );
    eprintln!(
        "  3. camp watch --config {} --write-state /tmp/{}-camp-state.json",
        config_path.display(),
        slugify(&config.agent.project)
    );

    Ok(())
}

async fn run_up(command: UpCommand) -> Result<(), Box<dyn Error>> {
    let config = read_camp_config(&command.config)?;
    let settings = AnnounceSettings::from(config);
    run_announce_with_settings(settings, command.duration_secs, command.json).await
}

async fn run_announce(command: AnnounceCommand) -> Result<(), Box<dyn Error>> {
    let settings = AnnounceSettings {
        id: command.id,
        role: command.role,
        project: command.project,
        branch: command.branch,
        port: command.port,
        status: command.status,
        capabilities: command.capabilities,
        metadata: command.metadata,
        service_type: command.service_type,
        mdns_port: command.mdns_port,
        heartbeat_ms: command.heartbeat_ms,
        ttl_ms: command.ttl_ms,
        shared_secret: command.shared_secret,
        shared_secret_accept: command.shared_secret_accept,
        shared_secret_mode: command.shared_secret_mode.into(),
        enable_interface: command.enable_interface,
        disable_interface: command.disable_interface,
    };
    run_announce_with_settings(settings, command.duration_secs, command.json).await
}

async fn run_list(command: ListCommand) -> Result<(), Box<dyn Error>> {
    let mesh = build_observer(&command.discovery).await?;
    let agents = discover_agents(&mesh, command.discovery.discover_ms).await;
    let filtered = agents
        .into_iter()
        .filter(|agent| matches_filters(agent, &command))
        .map(|agent| to_agent_record(&agent))
        .collect::<Vec<_>>();

    print_json_pretty(&filtered)?;
    mesh.shutdown().await?;
    Ok(())
}

async fn run_get(command: GetCommand) -> Result<(), Box<dyn Error>> {
    let mesh = build_observer(&command.discovery).await?;
    let agents = discover_agents(&mesh, command.discovery.discover_ms).await;
    let record = agents
        .into_iter()
        .find(|agent| agent.id() == command.id)
        .map(|agent| to_agent_record(&agent));

    print_json_pretty(&record)?;
    mesh.shutdown().await?;
    Ok(())
}

async fn run_watch(command: WatchCommand) -> Result<(), Box<dyn Error>> {
    let mesh = build_observer(&command.discovery).await?;
    time::sleep(Duration::from_millis(command.discovery.discover_ms)).await;

    let snapshot = SnapshotRecord {
        kind: "snapshot",
        agents: mesh.agents().await.iter().map(to_agent_record).collect(),
    };
    let snapshot_json = json_line_string(&snapshot)?;
    println!("{snapshot_json}");
    if let Some(path) = command.write_state.as_deref() {
        write_state_snapshot(path, &snapshot.agents)?;
    }
    if let Some(path) = command.write_events.as_deref() {
        append_json_line(path, &snapshot_json)?;
    }
    if let Some(exec) = command.exec.as_deref() {
        run_exec_hook(exec, "snapshot", &snapshot_json)?;
    }

    let mut events = mesh.subscribe();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        if let Some(record) = to_event_record(&event) {
                            let kind = record.kind;
                            let record_json = json_line_string(&record)?;
                            println!("{record_json}");
                            if let Some(path) = command.write_state.as_deref() {
                                let agents = mesh.agents().await.iter().map(to_agent_record).collect::<Vec<_>>();
                                write_state_snapshot(path, &agents)?;
                            }
                            if let Some(path) = command.write_events.as_deref() {
                                append_json_line(path, &record_json)?;
                            }
                            if let Some(exec) = command.exec.as_deref() {
                                run_exec_hook(exec, kind, &record_json)?;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    mesh.shutdown().await?;
    Ok(())
}

async fn run_serve(command: ServeCommand) -> Result<(), Box<dyn Error>> {
    let bind_addr = command.bind.parse::<std::net::SocketAddr>()?;
    let mesh = Arc::new(build_observer(&command.discovery).await?);
    let state = ServeState {
        mesh: Arc::clone(&mesh),
    };

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/agents", get(list_agents_handler))
        .route("/agents/{id}", get(get_agent_handler))
        .route("/events", get(events_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    eprintln!("camp: serving local mesh bridge on http://{bind_addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    mesh.shutdown().await?;
    Ok(())
}

fn run_completions(command: CompletionsCommand) -> Result<(), Box<dyn Error>> {
    let mut cli = Cli::command();
    generate(command.shell, &mut cli, "camp", &mut std::io::stdout());
    Ok(())
}

async fn build_observer(options: &DiscoveryOptions) -> Result<ZeroConfMesh, Box<dyn Error>> {
    let resolved = resolve_discovery_options(options)?;
    let mut builder = ZeroConfMesh::builder()
        .agent_id(format!("camp-observer-{}", uuid::Uuid::new_v4()))
        .role("observer")
        .project("observer")
        .branch("watch")
        .port(ephemeral_udp_port())
        .mdns_port(resolved.mdns_port)
        .service_type(resolved.service_type)
        .discover_only()
        .heartbeat_interval(Duration::from_millis(200))
        .ttl(Duration::from_secs(2));

    builder = apply_shared_secret(
        builder,
        resolved.shared_secret,
        resolved.shared_secret_accept,
        resolved.shared_secret_mode,
    );
    builder = apply_interfaces(
        builder,
        resolved.enable_interface,
        resolved.disable_interface,
    )?;

    Ok(builder.build().await?)
}

async fn discover_agents(mesh: &ZeroConfMesh, discover_ms: u64) -> Vec<AgentInfo> {
    time::sleep(Duration::from_millis(discover_ms)).await;
    mesh.agents().await
}

async fn run_announce_with_settings(
    settings: AnnounceSettings,
    duration_secs: Option<u64>,
    json: bool,
) -> Result<(), Box<dyn Error>> {
    let mut builder = ZeroConfMesh::builder()
        .agent_id(settings.id)
        .role(settings.role)
        .project(settings.project)
        .branch(settings.branch)
        .port(settings.port)
        .mdns_port(settings.mdns_port)
        .service_type(settings.service_type)
        .status(settings.status)
        .heartbeat_interval(Duration::from_millis(settings.heartbeat_ms))
        .ttl(Duration::from_millis(settings.ttl_ms))
        .capabilities(settings.capabilities);

    builder = apply_metadata(builder, settings.metadata);
    builder = apply_shared_secret(
        builder,
        settings.shared_secret,
        settings.shared_secret_accept,
        settings.shared_secret_mode,
    );
    builder = apply_interfaces(
        builder,
        settings.enable_interface,
        settings.disable_interface,
    )?;

    let mesh = builder.build().await?;

    eprintln!(
        "camp: announcing {} on {} ({})",
        mesh.local_agent_id(),
        mesh.config().project(),
        mesh.config().branch()
    );

    if json {
        print_json_pretty(&to_agent_record(
            &mesh
                .registry()
                .get(mesh.local_agent_id())
                .await
                .ok_or("local agent missing from registry")?,
        ))?;
    }

    if let Some(duration_secs) = duration_secs {
        time::sleep(Duration::from_secs(duration_secs)).await;
    } else {
        tokio::signal::ctrl_c().await?;
    }

    mesh.shutdown().await?;
    Ok(())
}

fn resolve_discovery_options(
    options: &DiscoveryOptions,
) -> Result<ResolvedDiscoveryOptions, Box<dyn Error>> {
    if let Some(path) = options.config.as_deref() {
        let config = read_camp_config(path)?;
        return Ok(ResolvedDiscoveryOptions {
            service_type: config.discovery.service_type,
            mdns_port: config.discovery.mdns_port,
            shared_secret: config.discovery.shared_secret,
            shared_secret_accept: config.discovery.shared_secret_accept,
            shared_secret_mode: config.discovery.shared_secret_mode.into(),
            enable_interface: config.discovery.enable_interface,
            disable_interface: config.discovery.disable_interface,
        });
    }

    Ok(ResolvedDiscoveryOptions {
        service_type: options.service_type.clone(),
        mdns_port: options.mdns_port,
        shared_secret: options.shared_secret.clone(),
        shared_secret_accept: options.shared_secret_accept.clone(),
        shared_secret_mode: options.shared_secret_mode.into(),
        enable_interface: options.enable_interface.clone(),
        disable_interface: options.disable_interface.clone(),
    })
}

fn matches_filters(agent: &AgentInfo, command: &ListCommand) -> bool {
    if let Some(id) = &command.id
        && agent.id() != id
    {
        return false;
    }
    if let Some(role) = &command.role
        && agent.role() != role
    {
        return false;
    }
    if let Some(project) = &command.project
        && agent.project() != project
    {
        return false;
    }
    if let Some(branch) = &command.branch
        && agent.branch() != branch
    {
        return false;
    }
    if let Some(status) = command.status
        && agent.status() != status
    {
        return false;
    }
    if let Some(capability) = &command.capability
        && !agent.has_capability(capability)
    {
        return false;
    }
    if command.metadata.iter().any(|(key, value)| {
        agent
            .metadata()
            .get(key)
            .is_none_or(|stored| stored != value)
    }) {
        return false;
    }
    if command
        .metadata_keys
        .iter()
        .any(|key| !agent.metadata().contains_key(key))
    {
        return false;
    }
    if command.metadata_key_prefixes.iter().any(|prefix| {
        !agent
            .metadata()
            .keys()
            .any(|key| key.starts_with(prefix.as_str()))
    }) {
        return false;
    }
    if command.metadata_prefixes.iter().any(|(key, prefix)| {
        agent
            .metadata()
            .get(key)
            .is_none_or(|stored| !stored.starts_with(prefix))
    }) {
        return false;
    }
    if command.metadata_regexes.iter().any(|(key, pattern)| {
        let Ok(regex) = regex::Regex::new(pattern) else {
            return true;
        };
        agent
            .metadata()
            .get(key)
            .is_none_or(|stored| !regex.is_match(stored))
    }) {
        return false;
    }

    true
}

fn matches_agent_query(agent: &AgentInfo, query: &AgentQuery) -> bool {
    if let Some(id) = &query.id
        && agent.id() != id
    {
        return false;
    }
    if let Some(role) = &query.role
        && agent.role() != role
    {
        return false;
    }
    if let Some(project) = &query.project
        && agent.project() != project
    {
        return false;
    }
    if let Some(branch) = &query.branch
        && agent.branch() != branch
    {
        return false;
    }
    if let Some(status) = &query.status
        && agent.status().as_str() != status
    {
        return false;
    }
    if let Some(capability) = &query.capability
        && !agent.has_capability(capability)
    {
        return false;
    }

    true
}

fn apply_metadata(
    mut builder: coding_agent_mesh_presence::ZeroConfMeshBuilder,
    metadata: Vec<(String, String)>,
) -> coding_agent_mesh_presence::ZeroConfMeshBuilder {
    for (key, value) in metadata {
        builder = builder.metadata(key, value);
    }
    builder
}

fn apply_shared_secret(
    builder: coding_agent_mesh_presence::ZeroConfMeshBuilder,
    shared_secret: Option<String>,
    shared_secret_accept: Vec<String>,
    mode: SharedSecretMode,
) -> coding_agent_mesh_presence::ZeroConfMeshBuilder {
    match shared_secret {
        Some(secret) if shared_secret_accept.is_empty() => {
            builder.shared_secret_with_mode(secret, mode)
        }
        Some(secret) => {
            builder.shared_secret_rotation_with_mode(secret, shared_secret_accept, mode)
        }
        None => builder,
    }
}

fn apply_interfaces(
    mut builder: coding_agent_mesh_presence::ZeroConfMeshBuilder,
    enabled: Vec<String>,
    disabled: Vec<String>,
) -> Result<coding_agent_mesh_presence::ZeroConfMeshBuilder, Box<dyn Error>> {
    for interface in enabled {
        builder = builder.enable_interface(parse_network_interface(&interface)?);
    }
    for interface in disabled {
        builder = builder.disable_interface(parse_network_interface(&interface)?);
    }
    Ok(builder)
}

fn to_agent_record(agent: &AgentInfo) -> AgentRecord {
    AgentRecord {
        id: agent.id().to_owned(),
        instance_name: agent.instance_name().to_owned(),
        role: agent.role().to_owned(),
        project: agent.project().to_owned(),
        branch: agent.branch().to_owned(),
        status: agent.status().as_str().to_owned(),
        capabilities: agent.capabilities().to_vec(),
        port: agent.port(),
        addresses: agent.addresses().iter().map(ToString::to_string).collect(),
        metadata: agent.metadata().clone(),
    }
}

fn to_event_record(event: &AgentEvent) -> Option<EventRecord> {
    match event {
        AgentEvent::Joined { agent, origin } => Some(EventRecord {
            kind: "joined",
            origin: origin_label(*origin),
            reason: None,
            previous: None,
            current: Some(to_agent_record(agent)),
        }),
        AgentEvent::Updated {
            previous,
            current,
            origin,
        } => Some(EventRecord {
            kind: "updated",
            origin: origin_label(*origin),
            reason: None,
            previous: Some(to_agent_record(previous)),
            current: Some(to_agent_record(current)),
        }),
        AgentEvent::Left {
            agent,
            origin,
            reason,
        } => Some(EventRecord {
            kind: "left",
            origin: origin_label(*origin),
            reason: Some(reason_label(*reason)),
            previous: None,
            current: Some(to_agent_record(agent)),
        }),
        _ => None,
    }
}

fn origin_label(origin: EventOrigin) -> &'static str {
    match origin {
        EventOrigin::Local => "local",
        EventOrigin::Remote => "remote",
    }
}

fn reason_label(reason: DepartureReason) -> &'static str {
    match reason {
        DepartureReason::Graceful => "graceful",
        DepartureReason::Expired => "expired",
    }
}

async fn health_handler() -> Json<HealthRecord> {
    Json(HealthRecord { ok: true })
}

async fn list_agents_handler(
    State(state): State<ServeState>,
    Query(query): Query<AgentQuery>,
) -> Json<Vec<AgentRecord>> {
    let agents = state
        .mesh
        .agents()
        .await
        .into_iter()
        .filter(|agent| matches_agent_query(agent, &query))
        .map(|agent| to_agent_record(&agent))
        .collect::<Vec<_>>();
    Json(agents)
}

async fn get_agent_handler(
    State(state): State<ServeState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.mesh.get_agent(&id).await {
        Some(agent) => (StatusCode::OK, Json(to_agent_record(&agent))).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn events_handler(State(state): State<ServeState>) -> impl IntoResponse {
    let mesh = Arc::clone(&state.mesh);
    let mut events = mesh.subscribe();

    let stream = stream! {
        let snapshot = SnapshotRecord {
            kind: "snapshot",
            agents: mesh.agents().await.iter().map(to_agent_record).collect(),
        };
        if let Ok(data) = serde_json::to_string(&snapshot) {
            yield Ok::<SseEvent, std::convert::Infallible>(
                SseEvent::default().event("snapshot").data(data)
            );
        }

        while let Ok(event) = events.recv().await {
            if let Some(record) = to_event_record(&event)
                && let Ok(data) = serde_json::to_string(&record)
            {
                yield Ok::<SseEvent, std::convert::Infallible>(
                    SseEvent::default().event(record.kind).data(data)
                );
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn parse_status(value: &str) -> Result<AgentStatus, String> {
    AgentStatus::from_str(value).map_err(|error| error.to_string())
}

fn parse_key_value(value: &str) -> Result<(String, String), String> {
    let Some((key, val)) = value.split_once('=') else {
        return Err("expected KEY=VALUE".to_owned());
    };
    Ok((key.trim().to_owned(), val.trim().to_owned()))
}

fn parse_network_interface(value: &str) -> Result<NetworkInterface, String> {
    match value {
        "all" => Ok(NetworkInterface::All),
        "ipv4" => Ok(NetworkInterface::IPv4),
        "ipv6" => Ok(NetworkInterface::IPv6),
        "loopback-v4" => Ok(NetworkInterface::LoopbackV4),
        "loopback-v6" => Ok(NetworkInterface::LoopbackV6),
        _ => {
            let Some((kind, raw)) = value.split_once(':') else {
                return Ok(NetworkInterface::Name(value.to_owned()));
            };
            match kind {
                "name" => Ok(NetworkInterface::Name(raw.to_owned())),
                "addr" => raw
                    .parse::<IpAddr>()
                    .map(NetworkInterface::Addr)
                    .map_err(|error| error.to_string()),
                "index-v4" => raw
                    .parse::<u32>()
                    .map(NetworkInterface::IndexV4)
                    .map_err(|error| error.to_string()),
                "index-v6" => raw
                    .parse::<u32>()
                    .map(NetworkInterface::IndexV6)
                    .map_err(|error| error.to_string()),
                _ => Err("unknown interface selector".to_owned()),
            }
        }
    }
}

fn infer_default_project() -> String {
    if let Some(package_name) = read_local_package_name() {
        return package_name;
    }

    std::env::current_dir()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_AGENT_PROJECT.to_owned())
}

fn infer_default_branch() -> String {
    try_command_stdout(&["git", "branch", "--show-current"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_AGENT_BRANCH.to_owned())
}

fn infer_default_agent_id(project: &str, branch: &str) -> String {
    let user = std::env::var("USER").ok().filter(|value| !value.is_empty());
    let host = std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| try_command_stdout(&["hostname"]).filter(|value| !value.is_empty()));
    let raw = [
        Some("camp".to_owned()),
        user,
        host,
        Some(project.to_owned()),
        Some(branch.to_owned()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("-");
    slugify(&raw)
}

fn slugify(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut last_dash = false;

    for ch in value.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            slug.push(normalized);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "camp-agent".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn try_command_stdout(argv: &[&str]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let output = ProcessCommand::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn read_local_package_name() -> Option<String> {
    let manifest = fs::read_to_string("Cargo.toml").ok()?;
    let value: toml::Value = toml::from_str(&manifest).ok()?;
    value
        .get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
        .filter(|name| !name.trim().is_empty())
}

fn read_camp_config(path: &Path) -> Result<CampConfigFile, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    Ok(toml::from_str(&contents)?)
}

fn write_camp_config(path: &Path, config: &CampConfigFile) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(config)?;
    fs::write(path, format!("{contents}\n"))?;
    Ok(())
}

fn upsert_agents_guidance(
    path: &Path,
    config_path: &Path,
    config: &CampConfigFile,
) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }

    let guidance = render_agents_guidance(config_path, config);
    let existing = fs::read_to_string(path).unwrap_or_default();
    let updated = replace_marked_block(
        &existing,
        AGENTS_GUIDANCE_START,
        AGENTS_GUIDANCE_END,
        &guidance,
    );
    fs::write(path, updated)?;
    Ok(())
}

fn render_agents_guidance(config_path: &Path, config: &CampConfigFile) -> String {
    let state_file = format!("/tmp/{}-camp-state.json", slugify(&config.agent.project));
    let config_display = config_path.display();
    format!(
        "{AGENTS_GUIDANCE_START}\n## camp agent workflow\n\n\
This repository is configured to use `camp` for local LAN agent discovery.\n\n\
If `{config_display}` is missing on this machine, run `camp init --force` before using the commands below.\n\n\
Recommended commands for AI agents in this repo:\n\
- bring this repo's agent online: `camp up`\n\
- list peers for this project: `camp who --config {config_display} --project {project}`\n\
- find a reviewer quickly: `camp who --config {config_display} --project {project} --role reviewer`\n\
- mirror live mesh state to a file: `camp watch --config {config_display} --write-state {state_file}`\n\
- start the local HTTP + SSE bridge: `camp serve --config {config_display} --bind 127.0.0.1:9999`\n\n\
The generated config already includes this repo's defaults for project, branch, ports, and discovery settings.\n\
Prefer reusing a single long-running `camp up` process instead of starting multiple announcers for the same machine.\n\
{AGENTS_GUIDANCE_END}\n",
        project = config.agent.project,
    )
}

fn replace_marked_block(existing: &str, start: &str, end: &str, replacement: &str) -> String {
    let trimmed_replacement = replacement.trim_end();

    match (existing.find(start), existing.find(end)) {
        (Some(start_idx), Some(end_idx)) if end_idx >= start_idx => {
            let before = existing[..start_idx].trim_end();
            let after = existing[end_idx + end.len()..].trim_start();
            if before.is_empty() && after.is_empty() {
                format!("{trimmed_replacement}\n")
            } else if before.is_empty() {
                format!("{trimmed_replacement}\n\n{after}\n")
            } else if after.is_empty() {
                format!("{before}\n\n{trimmed_replacement}\n")
            } else {
                format!("{before}\n\n{trimmed_replacement}\n\n{after}\n")
            }
        }
        _ if existing.trim().is_empty() => format!("{trimmed_replacement}\n"),
        _ => format!("{}\n\n{trimmed_replacement}\n", existing.trim_end()),
    }
}

fn print_json_pretty<T: Serialize>(value: &T) -> Result<(), Box<dyn Error>> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn json_line_string<T: Serialize>(value: &T) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(value)?)
}

fn print_banner() {
    eprintln!(
        "\x1b[95m\
 ██████╗ █████╗ ███╗   ███╗██████╗ \n\
██╔════╝██╔══██╗████╗ ████║██╔══██╗\n\
██║     ███████║██╔████╔██║██████╔╝\n\
██║     ██╔══██║██║╚██╔╝██║██╔═══╝ \n\
╚██████╗██║  ██║██║ ╚═╝ ██║██║     \n\
 ╚═════╝╚═╝  ╚═╝╚═╝     ╚═╝╚═╝     \n\
\x1b[94m╔════════════════════════════════════════════════╗\n\
║ coding agent mesh presence • shell-first JSON ║\n\
╚════════════════════════════════════════════════╝\x1b[0m\n"
    );
}

fn write_state_snapshot(path: &Path, agents: &[AgentRecord]) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }

    let payload = serde_json::to_vec_pretty(agents)?;
    let tmp_path = temporary_state_path(path);
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn append_json_line(path: &Path, line: &str) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn run_exec_hook(command: &str, kind: &str, payload: &str) -> Result<(), Box<dyn Error>> {
    let mut child = ProcessCommand::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .env("CAMP_KIND", kind)
        .env("CAMP_EVENT_JSON", payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes())?;
        stdin.write_all(b"\n")?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("camp exec hook failed with status {status}").into())
    }
}

fn temporary_state_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("camp-state.json");
    let tmp_name = format!(".{file_name}.tmp");
    path.with_file_name(tmp_name)
}

fn ephemeral_udp_port() -> u16 {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("ephemeral UDP port should be allocated")
        .local_addr()
        .expect("local address should be available")
        .port()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn key_value_parser_should_accept_equals_form() {
        let parsed = parse_key_value("capability=review").expect("key/value should parse");
        assert_eq!(parsed, ("capability".to_owned(), "review".to_owned()));
    }

    #[test]
    fn key_value_parser_should_reject_missing_separator() {
        assert!(parse_key_value("capability").is_err());
    }

    #[test]
    fn network_interface_parser_should_handle_named_and_special_values() {
        assert!(matches!(
            parse_network_interface("loopback-v4").expect("interface should parse"),
            NetworkInterface::LoopbackV4
        ));
        assert!(matches!(
            parse_network_interface("name:en0").expect("interface should parse"),
            NetworkInterface::Name(name) if name == "en0"
        ));
    }

    #[test]
    fn temporary_state_path_should_stay_next_to_target_file() {
        let path = Path::new("/tmp/agent_mesh_state.json");
        assert_eq!(
            temporary_state_path(path),
            PathBuf::from("/tmp/.agent_mesh_state.json.tmp")
        );
    }

    #[test]
    fn append_json_line_should_write_jsonl() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = env::temp_dir().join(format!("camp-events-{unique}.jsonl"));
        let payload = SnapshotRecord {
            kind: "snapshot",
            agents: Vec::new(),
        };
        let line = json_line_string(&payload).expect("json should serialize");

        append_json_line(&path, &line).expect("json line should be written");

        let contents = fs::read_to_string(&path).expect("json line file should exist");
        assert!(contents.ends_with('\n'));
        assert!(contents.contains("\"kind\":\"snapshot\""));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn run_exec_hook_should_pipe_json_to_stdin() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = env::temp_dir().join(format!("camp-hook-{unique}.json"));
        let payload = r#"{"kind":"snapshot"}"#;
        let command = format!("cat > {}", path.display());

        run_exec_hook(&command, "snapshot", payload).expect("hook should succeed");

        let contents = fs::read_to_string(&path).expect("hook output should exist");
        assert_eq!(contents, format!("{payload}\n"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn replace_marked_block_should_replace_existing_camp_guidance() {
        let existing = "# Project\n\nold\n<!-- CAMP:START -->\nold block\n<!-- CAMP:END -->\n";
        let replacement = "<!-- CAMP:START -->\nnew block\n<!-- CAMP:END -->";

        let updated = replace_marked_block(
            existing,
            AGENTS_GUIDANCE_START,
            AGENTS_GUIDANCE_END,
            replacement,
        );

        assert!(updated.contains("new block"));
        assert!(!updated.contains("old block"));
        assert!(updated.contains("# Project"));
    }

    #[test]
    fn camp_config_should_round_trip_through_toml() {
        let config = CampConfigFile {
            agent: CampAgentConfig {
                id: "coder-01".to_owned(),
                role: "reviewer".to_owned(),
                project: "alpha".to_owned(),
                branch: "main".to_owned(),
                port: 7000,
                status: AgentStatus::Idle,
                capabilities: vec!["review".to_owned()],
                metadata: BTreeMap::from([(String::from("team"), String::from("core"))]),
            },
            discovery: CampDiscoveryConfig {
                service_type: coding_agent_mesh_presence::DEFAULT_SERVICE_TYPE.to_owned(),
                mdns_port: coding_agent_mesh_presence::DEFAULT_MDNS_PORT,
                heartbeat_ms: DEFAULT_HEARTBEAT_MS,
                ttl_ms: DEFAULT_TTL_MS,
                shared_secret: Some("secret".to_owned()),
                shared_secret_accept: vec!["old-secret".to_owned()],
                shared_secret_mode: SharedSecretModeArg::SignAndVerify,
                enable_interface: vec!["ipv4".to_owned()],
                disable_interface: vec!["loopback-v4".to_owned()],
            },
        };

        let serialized = toml::to_string_pretty(&config).expect("config should serialize");
        let round_trip: CampConfigFile =
            toml::from_str(&serialized).expect("config should deserialize");

        assert_eq!(round_trip.agent.id, "coder-01");
        assert_eq!(round_trip.agent.capabilities, vec!["review"]);
        assert_eq!(
            round_trip.discovery.shared_secret.as_deref(),
            Some("secret")
        );
    }

    #[test]
    fn infer_default_project_should_prefer_manifest_name() {
        assert_eq!(
            infer_default_project(),
            "coding_agent_mesh_presence".to_owned()
        );
    }
}
