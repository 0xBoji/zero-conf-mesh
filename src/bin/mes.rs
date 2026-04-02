//! `mes` is the command-line interface for `zero-conf-mesh`.
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
    time::Duration,
};

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use serde::Serialize;
use tokio::time;
use zero_conf_mesh::{
    AgentEvent, AgentInfo, AgentStatus, DepartureReason, EventOrigin, NetworkInterface,
    SharedSecretMode, ZeroConfMesh,
};

#[derive(Parser, Debug)]
#[command(name = "mes", version, about = "Zero-conf agent discovery CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Announce a local agent on the LAN and keep it online until interrupted.
    Announce(AnnounceCommand),
    /// Discover peers on the LAN and print the current registry as JSON.
    #[command(alias = "who")]
    List(ListCommand),
    /// Discover a single peer by id and print it as JSON.
    Get(GetCommand),
    /// Watch discovery events and print newline-delimited JSON.
    Watch(WatchCommand),
    /// Generate shell completion scripts for mes.
    Completions(CompletionsCommand),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum SharedSecretModeArg {
    SignOnly,
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
    /// Service type to browse.
    #[arg(long, default_value = zero_conf_mesh::DEFAULT_SERVICE_TYPE)]
    service_type: String,
    /// UDP port used by the embedded mDNS daemon.
    #[arg(long, default_value_t = zero_conf_mesh::DEFAULT_MDNS_PORT)]
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
    #[arg(long, default_value = zero_conf_mesh::DEFAULT_SERVICE_TYPE)]
    service_type: String,
    /// UDP port used by the embedded mDNS daemon.
    #[arg(long, default_value_t = zero_conf_mesh::DEFAULT_MDNS_PORT)]
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    print_banner();
    let cli = Cli::parse();

    match cli.command {
        Command::Announce(command) => run_announce(command).await?,
        Command::List(command) => run_list(command).await?,
        Command::Get(command) => run_get(command).await?,
        Command::Watch(command) => run_watch(command).await?,
        Command::Completions(command) => run_completions(command)?,
    }

    Ok(())
}

async fn run_announce(command: AnnounceCommand) -> Result<(), Box<dyn Error>> {
    let mut builder = ZeroConfMesh::builder()
        .agent_id(command.id)
        .role(command.role)
        .project(command.project)
        .branch(command.branch)
        .port(command.port)
        .mdns_port(command.mdns_port)
        .service_type(command.service_type)
        .status(command.status)
        .heartbeat_interval(Duration::from_millis(command.heartbeat_ms))
        .ttl(Duration::from_millis(command.ttl_ms))
        .capabilities(command.capabilities);

    builder = apply_metadata(builder, command.metadata);
    builder = apply_shared_secret(
        builder,
        command.shared_secret,
        command.shared_secret_accept,
        command.shared_secret_mode.into(),
    );
    builder = apply_interfaces(builder, command.enable_interface, command.disable_interface)?;

    let mesh = builder.build().await?;

    eprintln!(
        "mes: announcing {} on {} ({})",
        mesh.local_agent_id(),
        mesh.config().project(),
        mesh.config().branch()
    );

    if command.json {
        print_json_pretty(&to_agent_record(
            &mesh
                .registry()
                .get(mesh.local_agent_id())
                .await
                .ok_or("local agent missing from registry")?,
        ))?;
    }

    if let Some(duration_secs) = command.duration_secs {
        time::sleep(Duration::from_secs(duration_secs)).await;
    } else {
        tokio::signal::ctrl_c().await?;
    }

    mesh.shutdown().await?;
    Ok(())
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

fn run_completions(command: CompletionsCommand) -> Result<(), Box<dyn Error>> {
    let mut cli = Cli::command();
    generate(command.shell, &mut cli, "mes", &mut std::io::stdout());
    Ok(())
}

async fn build_observer(options: &DiscoveryOptions) -> Result<ZeroConfMesh, Box<dyn Error>> {
    let mut builder = ZeroConfMesh::builder()
        .agent_id(format!("mes-observer-{}", uuid::Uuid::new_v4()))
        .role("observer")
        .project("observer")
        .branch("watch")
        .port(ephemeral_udp_port())
        .mdns_port(options.mdns_port)
        .service_type(options.service_type.clone())
        .discover_only()
        .heartbeat_interval(Duration::from_millis(200))
        .ttl(Duration::from_secs(2));

    builder = apply_shared_secret(
        builder,
        options.shared_secret.clone(),
        options.shared_secret_accept.clone(),
        options.shared_secret_mode.into(),
    );
    builder = apply_interfaces(
        builder,
        options.enable_interface.clone(),
        options.disable_interface.clone(),
    )?;

    Ok(builder.build().await?)
}

async fn discover_agents(mesh: &ZeroConfMesh, discover_ms: u64) -> Vec<AgentInfo> {
    time::sleep(Duration::from_millis(discover_ms)).await;
    mesh.agents().await
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

fn apply_metadata(
    mut builder: zero_conf_mesh::ZeroConfMeshBuilder,
    metadata: Vec<(String, String)>,
) -> zero_conf_mesh::ZeroConfMeshBuilder {
    for (key, value) in metadata {
        builder = builder.metadata(key, value);
    }
    builder
}

fn apply_shared_secret(
    builder: zero_conf_mesh::ZeroConfMeshBuilder,
    shared_secret: Option<String>,
    shared_secret_accept: Vec<String>,
    mode: SharedSecretMode,
) -> zero_conf_mesh::ZeroConfMeshBuilder {
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
    mut builder: zero_conf_mesh::ZeroConfMeshBuilder,
    enabled: Vec<String>,
    disabled: Vec<String>,
) -> Result<zero_conf_mesh::ZeroConfMeshBuilder, Box<dyn Error>> {
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
███╗   ███╗███████╗ ██████╗ \n\
████╗ ████║██╔════╝██╔════╝ \n\
██╔████╔██║█████╗  ╚█████╗  \n\
██║╚██╔╝██║██╔══╝   ╚═══██╗ \n\
██║ ╚═╝ ██║███████╗██████╔╝ \n\
╚═╝     ╚═╝╚══════╝╚═════╝  \n\
\x1b[94m╔══════════════════════════════════════════════╗\n\
║  zero-conf mesh agent cli • shell-first JSON ║\n\
╚══════════════════════════════════════════════╝\x1b[0m\n"
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
        .env("MES_KIND", kind)
        .env("MES_EVENT_JSON", payload)
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
        Err(format!("mes exec hook failed with status {status}").into())
    }
}

fn temporary_state_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mes-state.json");
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
        let path = env::temp_dir().join(format!("mes-events-{unique}.jsonl"));
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
        let path = env::temp_dir().join(format!("mes-hook-{unique}.json"));
        let payload = r#"{"kind":"snapshot"}"#;
        let command = format!("cat > {}", path.display());

        run_exec_hook(&command, "snapshot", payload).expect("hook should succeed");

        let contents = fs::read_to_string(&path).expect("hook output should exist");
        assert_eq!(contents, format!("{payload}\n"));

        let _ = fs::remove_file(path);
    }
}
