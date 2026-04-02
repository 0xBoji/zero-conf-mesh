//! Runtime integration tests for multi-node mesh discovery behavior.

use std::{
    error::Error,
    net::{Ipv4Addr, UdpSocket},
    time::Duration,
};

use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::time;
use zero_conf_mesh::{AgentStatus, DEFAULT_SERVICE_TYPE, ZeroConfMesh};

#[tokio::test]
async fn mesh_should_propagate_status_updates_to_remote_peers() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_a = mesh("agent-a", "alpha", "main", 8081, mdns_port).await?;
    let mesh_b = mesh("agent-b", "alpha", "main", 8082, mdns_port).await?;

    wait_for_agent(&mesh_b, "agent-a").await?;

    mesh_a.update_status(AgentStatus::Busy).await?;

    let peer = wait_for_agent_with_status(&mesh_b, "agent-a", AgentStatus::Busy).await?;

    assert_eq!(peer.status(), AgentStatus::Busy);

    mesh_a.shutdown().await?;
    mesh_b.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_propagate_project_branch_and_metadata_updates() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_a = mesh("agent-a", "alpha", "main", 8081, mdns_port).await?;
    let mesh_b = mesh("agent-b", "alpha", "main", 8082, mdns_port).await?;

    wait_for_agent(&mesh_b, "agent-a").await?;

    mesh_a.update_project("beta").await?;
    mesh_a.update_branch("feature/runtime").await?;
    mesh_a.update_metadata("capability", "planning").await?;

    let peer = wait_for_agent_matching(&mesh_b, "agent-a", |agent| {
        agent.project() == "beta"
            && agent.branch() == "feature/runtime"
            && agent.metadata().get("capability") == Some(&"planning".to_owned())
    })
    .await?;

    assert_eq!(peer.project(), "beta");
    assert_eq!(peer.branch(), "feature/runtime");
    assert_eq!(
        peer.metadata().get("capability"),
        Some(&"planning".to_owned())
    );

    mesh_a.shutdown().await?;
    mesh_b.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_propagate_metadata_removals_and_role_queries() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_a = ZeroConfMesh::builder()
        .agent_id("agent-a")
        .role("planner")
        .project("alpha")
        .branch("main")
        .port(8081)
        .mdns_port(mdns_port)
        .heartbeat_interval(Duration::from_millis(200))
        .ttl(Duration::from_secs(2))
        .metadata("capability", "planning")
        .build()
        .await?;
    let mesh_b = mesh("agent-b", "alpha", "main", 8082, mdns_port).await?;

    wait_for_agent(&mesh_b, "agent-a").await?;

    assert_eq!(
        ids(&mesh_b.agents_by_role("planner").await),
        vec!["agent-a"]
    );
    assert_eq!(
        ids(&mesh_b.agents_with_metadata("capability", "planning").await),
        vec!["agent-a"]
    );

    mesh_a.remove_metadata("capability").await?;

    wait_for_agent_matching(&mesh_b, "agent-a", |agent| {
        agent.metadata().get("capability").is_none()
    })
    .await?;

    assert!(
        mesh_b
            .agents_with_metadata("capability", "planning")
            .await
            .is_empty()
    );

    mesh_a.shutdown().await?;
    mesh_b.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_discover_multiple_peers_on_same_mdns_port() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_a = mesh("agent-a", "alpha", "main", 8081, mdns_port).await?;
    let mesh_b = mesh("agent-b", "alpha", "feature/mesh", 8082, mdns_port).await?;
    let mesh_c = mesh("agent-c", "beta", "main", 8083, mdns_port).await?;

    wait_for_registry_size(&mesh_a, 3).await?;
    wait_for_registry_size(&mesh_b, 3).await?;
    wait_for_registry_size(&mesh_c, 3).await?;

    assert_eq!(
        ids(&mesh_a.agents().await),
        vec!["agent-a", "agent-b", "agent-c"]
    );
    assert_eq!(
        ids(&mesh_a.agents_by_project("alpha").await),
        vec!["agent-a", "agent-b"]
    );
    assert_eq!(
        ids(&mesh_a.agents_by_branch("main").await),
        vec!["agent-a", "agent-c"]
    );

    mesh_a.shutdown().await?;
    mesh_b.shutdown().await?;
    mesh_c.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_filter_projects_independently_on_shared_lan() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_alpha = mesh("agent-alpha", "alpha", "main", 8081, mdns_port).await?;
    let mesh_beta = mesh("agent-beta", "beta", "main", 8082, mdns_port).await?;

    wait_for_agent(&mesh_alpha, "agent-beta").await?;

    assert_eq!(
        ids(&mesh_alpha.agents_by_project("alpha").await),
        vec!["agent-alpha"]
    );
    assert_eq!(
        ids(&mesh_alpha.agents_by_project("beta").await),
        vec!["agent-beta"]
    );
    assert_eq!(
        ids(&mesh_alpha
            .agents_by_project_and_branch("alpha", "main")
            .await),
        vec!["agent-alpha"]
    );
    assert_eq!(
        ids(&mesh_alpha
            .agents_by_project_and_branch("beta", "main")
            .await),
        vec!["agent-beta"]
    );

    mesh_alpha.shutdown().await?;
    mesh_beta.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_ignore_remote_services_with_malformed_txt_payloads()
-> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let listener_mesh = mesh("agent-listener", "alpha", "main", 8081, mdns_port).await?;
    let valid_mesh = mesh("agent-valid", "alpha", "main", 8082, mdns_port).await?;

    let invalid_daemon = ServiceDaemon::new_with_port(mdns_port)?;
    let invalid_service = ServiceInfo::new(
        DEFAULT_SERVICE_TYPE,
        "agent-invalid",
        "agent-invalid.local.",
        "",
        8083,
        &[
            ("agent_id", "agent-invalid"),
            ("role", "worker"),
            ("current_branch", "main"),
            ("status", "busy"),
        ][..],
    )?
    .enable_addr_auto();
    let invalid_fullname = invalid_service.get_fullname().to_string();
    invalid_daemon.register(invalid_service)?;

    wait_for_agent(&listener_mesh, "agent-valid").await?;
    wait_for_agent_to_remain_absent(&listener_mesh, "agent-invalid", Duration::from_secs(2))
        .await?;

    assert_eq!(
        ids(&listener_mesh.agents().await),
        vec!["agent-listener", "agent-valid"]
    );

    unregister_service(&invalid_daemon, &invalid_fullname).await;
    let _ = invalid_daemon.shutdown();
    listener_mesh.shutdown().await?;
    valid_mesh.shutdown().await?;

    Ok(())
}

async fn mesh(
    agent_id: &str,
    project: &str,
    branch: &str,
    port: u16,
    mdns_port: u16,
) -> Result<ZeroConfMesh, zero_conf_mesh::ZeroConfError> {
    ZeroConfMesh::builder()
        .agent_id(agent_id)
        .role("worker")
        .project(project)
        .branch(branch)
        .port(port)
        .mdns_port(mdns_port)
        .heartbeat_interval(Duration::from_millis(200))
        .ttl(Duration::from_secs(2))
        .build()
        .await
}

async fn wait_for_agent(mesh: &ZeroConfMesh, agent_id: &str) -> Result<(), Box<dyn Error>> {
    let _ = wait_for_agent_matching(mesh, agent_id, |_| true).await?;
    Ok(())
}

async fn wait_for_agent_with_status(
    mesh: &ZeroConfMesh,
    agent_id: &str,
    status: AgentStatus,
) -> Result<zero_conf_mesh::AgentInfo, Box<dyn Error>> {
    wait_for_agent_matching(mesh, agent_id, |agent| agent.status() == status).await
}

async fn wait_for_agent_matching<F>(
    mesh: &ZeroConfMesh,
    agent_id: &str,
    predicate: F,
) -> Result<zero_conf_mesh::AgentInfo, Box<dyn Error>>
where
    F: Fn(&zero_conf_mesh::AgentInfo) -> bool,
{
    let deadline = time::Instant::now() + Duration::from_secs(5);
    while time::Instant::now() < deadline {
        if let Some(agent) = mesh.get_agent(agent_id).await
            && predicate(&agent)
        {
            return Ok(agent);
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("timed out waiting for agent `{agent_id}`").into())
}

async fn wait_for_registry_size(
    mesh: &ZeroConfMesh,
    expected: usize,
) -> Result<(), Box<dyn Error>> {
    let deadline = time::Instant::now() + Duration::from_secs(5);
    while time::Instant::now() < deadline {
        if mesh.agents().await.len() == expected {
            return Ok(());
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("timed out waiting for registry size {expected}").into())
}

async fn wait_for_agent_to_remain_absent(
    mesh: &ZeroConfMesh,
    agent_id: &str,
    duration: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = time::Instant::now() + duration;
    while time::Instant::now() < deadline {
        if mesh.get_agent(agent_id).await.is_some() {
            return Err(format!("agent `{agent_id}` should have been ignored").into());
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

fn ids(agents: &[zero_conf_mesh::AgentInfo]) -> Vec<&str> {
    agents.iter().map(|agent| agent.id()).collect()
}

async fn unregister_service(daemon: &ServiceDaemon, fullname: &str) {
    if let Ok(receiver) = daemon.unregister(fullname) {
        let _ = receiver.recv_async().await;
    }
}

fn available_udp_port() -> u16 {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("ephemeral UDP port should be allocated")
        .local_addr()
        .expect("local address should be available")
        .port()
}
