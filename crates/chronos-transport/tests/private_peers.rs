use attow_transport::{
    DenyProposals, ObjectRepository, PeerConfig, PeerNode, PeerPermissions, Scope, SecretKey,
    SqliteObjectRepository, TransferObject, TransportLimits,
};
use chronos_codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id};
use chronos_store::Store;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
async fn read_wire(
    recv: &mut iroh::endpoint::RecvStream,
) -> std::result::Result<chronos_protocol::Message, Box<dyn std::error::Error>> {
    let mut header = [0u8; 5];
    recv.read_exact(&mut header).await?;
    let limit = chronos_protocol::ProtocolLimits::default();
    let parsed = chronos_protocol::decode_header(&header, &limit)?;
    let mut bytes = vec![0; parsed.payload_bytes + 4];
    bytes[..5].copy_from_slice(&header);
    recv.read_exact(&mut bytes[5..]).await?;
    Ok(chronos_protocol::decode_frame(&bytes, &limit)?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_or_replayed_connection_proof_cannot_invoke_authority() -> Result {
    use chronos_protocol::{ALPN, Hello, Message, PROTOCOL_VERSION, encode_frame};
    use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, endpoint::presets};
    let dir = tempfile::tempdir()?;
    let ka = SecretKey::from_bytes(&[81; 32]);
    let kb = SecretKey::from_bytes(&[82; 32]);
    let counter = Arc::new(CountingAuthority(std::sync::atomic::AtomicUsize::new(0)));
    let mut cfg = config(
        kb,
        &ka,
        Arc::new(SqliteObjectRepository::open(&dir.path().join("b.sqlite"))?),
    );
    cfg.permissions
        .values_mut()
        .for_each(|p| p.proposals = true);
    cfg.authority = counter.clone();
    let server = PeerNode::bind(cfg).await?;
    let client = Endpoint::builder(presets::Minimal)
        .secret_key(ka)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>()?)?
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    let address = EndpointAddr::new(EndpointId::from_bytes(&server.address().peer)?)
        .with_ip_addr(server.address().address);
    let greeting = Message::Hello(Hello {
        version: PROTOCOL_VERSION,
        tenant: scope().tenant,
        domain: scope().domain,
    });
    let mut previous = [0u8; 32];
    for case in 0..3 {
        let connection = client.connect(address.clone(), ALPN).await?;
        let (mut send, mut receive) = connection.open_bi().await?;
        send.write_all(&encode_frame(&greeting)?).await?;
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), read_wire(&mut receive))
                .await??,
            greeting
        );
        let mut context = [0u8; 64];
        context[..32].copy_from_slice(&scope().tenant.0);
        context[32..].copy_from_slice(&scope().domain.0);
        let mut current = [0u8; 32];
        connection
            .export_keying_material(&mut current, b"attow/1/handshake-confirmed", &context)
            .expect("completed TLS handshake exports connection proof");
        let proof = match case {
            0 => {
                previous = current;
                current
            }
            1 => [0u8; 32],
            _ => {
                assert_ne!(current, previous);
                previous
            }
        };
        send.write_all(&proof).await?;
        send.write_all(&encode_frame(&Message::BranchProposal(proposal()))?)
            .await?;
        send.finish()?;
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(5), read_wire(&mut receive))
                .await??;
        if case == 0 {
            assert!(matches!(response, Message::PromotionResult(_)));
        } else {
            assert!(matches!(response,Message::ProtocolError(error) if error.code==1));
        }
        assert_eq!(counter.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        connection.close(0u32.into(), b"test finished");
    }
    client.close().await;
    server.close().await;
    Ok(())
}

struct CountingAuthority(std::sync::atomic::AtomicUsize);
impl attow_transport::ProposalHandler for CountingAuthority {
    fn propose(
        &self,
        context: attow_transport::ProposalContext,
        p: chronos_protocol::ProposalEnvelope,
    ) -> std::result::Result<chronos_protocol::PromotionReply, attow_transport::TransportError>
    {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        TestAuthority.propose(context, p)
    }
}
fn proposal() -> chronos_protocol::ProposalEnvelope {
    chronos_protocol::ProposalEnvelope {
        request_id: ObjectId([61; 32]),
        tenant: scope().tenant,
        domain: scope().domain,
        epoch: 1,
        branch: ObjectId([62; 32]),
        payload: ObjectId([63; 32]),
        decision: ObjectId([64; 32]),
        generation: 0,
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutation_without_handshake_bound_proof_is_rejected() -> Result {
    use chronos_protocol::{ALPN, Hello, Message, PROTOCOL_VERSION, encode_frame};
    use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, endpoint::presets};
    let dir = tempfile::tempdir()?;
    let ka = SecretKey::from_bytes(&[71; 32]);
    let kb = SecretKey::from_bytes(&[72; 32]);
    let counter = Arc::new(CountingAuthority(std::sync::atomic::AtomicUsize::new(0)));
    let mut cfg = config(
        kb,
        &ka,
        Arc::new(SqliteObjectRepository::open(&dir.path().join("b.sqlite"))?),
    );
    cfg.permissions
        .values_mut()
        .for_each(|p| p.proposals = true);
    cfg.authority = counter.clone();
    let server = PeerNode::bind(cfg).await?;
    let client = Endpoint::builder(presets::Minimal)
        .secret_key(ka)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>()?)?
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    let address = EndpointAddr::new(EndpointId::from_bytes(&server.address().peer)?)
        .with_ip_addr(server.address().address);
    let connection = client.connect(address, ALPN).await?;
    let (mut send, mut receive) = connection.open_bi().await?;
    send.write_all(&encode_frame(&Message::Hello(Hello {
        version: PROTOCOL_VERSION,
        tenant: scope().tenant,
        domain: scope().domain,
    }))?)
    .await?;
    send.write_all(&encode_frame(&Message::BranchProposal(proposal()))?)
        .await?;
    send.finish()?;
    let _response =
        tokio::time::timeout(std::time::Duration::from_secs(5), receive.read_to_end(1024))
            .await??;
    connection.close(0u32.into(), b"test finished");
    client.close().await;
    server.close().await;
    assert_eq!(
        counter.0.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "unproved operation must never reach application"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_wire_rejects_wrong_alpn_version_and_oversized_headers() -> Result {
    use chronos_protocol::{ALPN, Hello, Message, PROTOCOL_VERSION, decode_frame, encode_frame};
    use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, endpoint::presets};
    use std::time::Duration;
    let dir = tempfile::tempdir()?;
    let repo = Arc::new(SqliteObjectRepository::open(&dir.path().join("b.sqlite"))?);
    let ka = SecretKey::from_bytes(&[51; 32]);
    let kb = SecretKey::from_bytes(&[52; 32]);
    let server = PeerNode::bind(config(kb, &ka, repo)).await?;
    let client = Endpoint::builder(presets::Minimal)
        .secret_key(ka)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>()?)?
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    let addr = EndpointAddr::new(EndpointId::from_bytes(&server.address().peer)?)
        .with_ip_addr(server.address().address);
    let wrong_alpn = tokio::time::timeout(
        Duration::from_secs(5),
        client.connect(addr.clone(), b"attow/2"),
    )
    .await?;
    assert!(wrong_alpn.is_err());
    let mut wrong_version = encode_frame(&Message::Hello(Hello {
        version: PROTOCOL_VERSION,
        tenant: scope().tenant,
        domain: scope().domain,
    }))?;
    assert_eq!(wrong_version[6], 1);
    wrong_version[6] = 2;
    for malformed in [
        vec![255, 255, 255, 255, 1],
        vec![0, 0, 0, 2, 255],
        vec![0, 0, 0, 9, 1],
        wrong_version,
    ] {
        let connection = client.connect(addr.clone(), ALPN).await?;
        let (mut send, mut receive) = connection.open_bi().await?;
        send.write_all(&malformed).await?;
        send.finish()?;
        let response =
            tokio::time::timeout(Duration::from_secs(5), receive.read_to_end(256)).await??;
        assert!(matches!(
            decode_frame(&response, &Default::default())?,
            Message::ProtocolError(_)
        ));
        connection.close(0u32.into(), b"test finished");
    }
    client.close().await;
    let stranger = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&[53; 32]))
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>()?)?
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    // The receiver rejects an unlisted authenticated key before any application bytes.
    if let Ok(connection) = stranger.connect(addr, ALPN).await {
        let reason = tokio::time::timeout(Duration::from_secs(5), connection.closed()).await?;
        assert!(matches!(
            reason,
            iroh::endpoint::ConnectionError::ApplicationClosed(_)
        ));
    }
    stranger.close().await;
    server.close().await;
    Ok(())
}
fn scope() -> Scope {
    Scope {
        tenant: ObjectId([7; 32]),
        domain: ObjectId([8; 32]),
    }
}
fn object(payload: Value, refs: BTreeSet<ObjectId>) -> TransferObject {
    let bytes = Object { payload, refs }.encode().expect("fixture");
    TransferObject {
        id: object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes),
        kind: OBJECT_KIND,
        schema: OBJECT_SCHEMA,
        bytes,
    }
}
#[test]
fn graph_import_commits_complete_pins_and_rejects_partial_or_corrupt_batches() -> Result {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("objects.sqlite");
    let repo = SqliteObjectRepository::open(&path)?;
    let child = object(Value::Text("owned".into()), BTreeSet::new());
    let root = object(Value::Null, BTreeSet::from([child.id]));
    assert!(
        repo.import(scope(), &[root.id], vec![root.clone()], 4096)
            .is_err()
    );
    assert!(!Store::open(&path)?.contains(root.id)?);
    let mut bad = child.clone();
    bad.bytes[0] ^= 1;
    assert!(
        repo.import(scope(), &[root.id], vec![bad, root.clone()], 4096)
            .is_err()
    );
    repo.import(scope(), &[root.id], vec![root.clone(), child.clone()], 4096)?;
    drop(repo);
    let repo = SqliteObjectRepository::open(&path)?;
    let exported = repo.export(scope(), &[root.id], 4096)?;
    assert_eq!(exported.len(), 2);
    Store::open(&path)?.collect_garbage()?;
    assert!(Store::open(&path)?.contains(root.id)?);
    assert!(
        repo.export(
            Scope {
                tenant: ObjectId([9; 32]),
                ..scope()
            },
            &[root.id],
            4096
        )
        .is_err()
    );
    Ok(())
}
fn config(key: SecretKey, other: &SecretKey, repo: Arc<dyn ObjectRepository>) -> PeerConfig {
    PeerConfig {
        secret_key: key,
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        tenant: scope().tenant,
        domain: scope().domain,
        permissions: BTreeMap::from([(
            *other.public().as_bytes(),
            PeerPermissions {
                objects: true,
                proposals: false,
            },
        )]),
        limits: TransportLimits::default(),
        objects: repo,
        authority: Arc::new(DenyProposals),
    }
}

#[test]
fn invalid_limits_fail_without_overflow_or_semaphore_allocation() {
    for (bulk_streams, control_streams) in [(usize::MAX, 1), (1, usize::MAX), (32, 1), (0, 1)] {
        assert!(
            TransportLimits {
                bulk_streams,
                control_streams,
                ..TransportLimits::default()
            }
            .validate()
            .is_err()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_peer_identity_scope_and_private_address_are_required() -> Result {
    let dir = tempfile::tempdir()?;
    let a = Arc::new(SqliteObjectRepository::open(&dir.path().join("a.sqlite"))?);
    let b = Arc::new(SqliteObjectRepository::open(&dir.path().join("b.sqlite"))?);
    let root = object(Value::Null, BTreeSet::new());
    a.import(scope(), &[root.id], vec![root.clone()], 4096)?;
    let ka = SecretKey::from_bytes(&[21; 32]);
    let kb = SecretKey::from_bytes(&[22; 32]);
    let mut bad = config(ka.clone(), &kb, a.clone());
    bad.bind_addr = "0.0.0.0:0".parse()?;
    assert!(matches!(
        PeerNode::bind(bad).await,
        Err(attow_transport::TransportError::Configuration)
    ));
    let node_a = PeerNode::bind(config(ka.clone(), &kb, a)).await?;
    let mut other_scope = config(kb.clone(), &ka, b.clone());
    other_scope.tenant = ObjectId([99; 32]);
    let node_b = PeerNode::bind(other_scope).await?;
    assert!(matches!(
        node_a.sync_objects(node_b.address(), &[root.id]).await,
        Err(attow_transport::TransportError::Unauthorized)
    ));
    assert!(b.export(scope(), &[root.id], 4096).is_err());
    let mut stranger = node_b.address();
    stranger.peer = *SecretKey::from_bytes(&[23; 32]).public().as_bytes();
    assert!(matches!(
        node_a.sync_objects(stranger, &[root.id]).await,
        Err(attow_transport::TransportError::Unauthorized)
    ));
    let mut public = node_b.address();
    public.address = "203.0.113.1:443".parse()?;
    assert!(matches!(
        node_a.sync_objects(public, &[root.id]).await,
        Err(attow_transport::TransportError::Configuration)
    ));
    assert!(matches!(
        node_a
            .sync_objects(node_b.address(), &vec![root.id; 4097])
            .await,
        Err(attow_transport::TransportError::Limit)
    ));
    node_a.close().await;
    node_b.close().await;
    Ok(())
}

struct TestAuthority;
impl attow_transport::ProposalHandler for TestAuthority {
    fn propose(
        &self,
        context: attow_transport::ProposalContext,
        p: chronos_protocol::ProposalEnvelope,
    ) -> std::result::Result<chronos_protocol::PromotionReply, attow_transport::TransportError>
    {
        assert_eq!((context.tenant, context.domain), (p.tenant, p.domain));
        Ok(chronos_protocol::PromotionReply {
            request_id: p.request_id,
            domain: p.domain,
            epoch: p.epoch,
            status: chronos_protocol::PromotionStatus::LocalCommitted { seq: 1 },
        })
    }
}
struct BlockedImport {
    inner: Arc<SqliteObjectRepository>,
    started: tokio::sync::Semaphore,
    released: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}
impl BlockedImport {
    fn release(&self) {
        *self.released.lock().expect("gate") = true;
        self.wake.notify_all();
    }
}
struct ReleaseOnDrop(Arc<BlockedImport>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
impl ObjectRepository for BlockedImport {
    fn export(
        &self,
        s: Scope,
        r: &[ObjectId],
        m: usize,
    ) -> std::result::Result<Vec<TransferObject>, attow_transport::TransportError> {
        self.inner.export(s, r, m)
    }
    fn import(
        &self,
        s: Scope,
        r: &[ObjectId],
        o: Vec<TransferObject>,
        m: usize,
    ) -> std::result::Result<u64, attow_transport::TransportError> {
        self.started.add_permits(1);
        let mut released = self.released.lock().expect("gate");
        while !*released {
            released = self.wake.wait(released).expect("gate");
        }
        drop(released);
        self.inner.import(s, r, o, m)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_bulk_import_cannot_ack_or_starve_control_and_quotas_release() -> Result {
    use attow_transport::TransportError;
    use chronos_protocol::{PromotionStatus, ProposalEnvelope};
    use std::time::Duration;
    let dir = tempfile::tempdir()?;
    let a = Arc::new(SqliteObjectRepository::open(&dir.path().join("a.sqlite"))?);
    let b = Arc::new(SqliteObjectRepository::open(&dir.path().join("b.sqlite"))?);
    let blocking = Arc::new(BlockedImport {
        inner: b.clone(),
        started: tokio::sync::Semaphore::new(0),
        released: std::sync::Mutex::new(false),
        wake: std::sync::Condvar::new(),
    });
    let _release = ReleaseOnDrop(blocking.clone());
    let root = object(Value::Bytes(vec![42; 100_000]), BTreeSet::new());
    a.import(scope(), &[root.id], vec![root.clone()], 200_000)?;
    let ka = SecretKey::from_bytes(&[31; 32]);
    let kb = SecretKey::from_bytes(&[32; 32]);
    let mut ca = config(ka.clone(), &kb, a);
    let mut cb = config(kb, &ka, blocking.clone());
    for c in [&mut ca, &mut cb] {
        c.limits.bulk_streams = 1;
        c.permissions.values_mut().for_each(|p| p.proposals = true);
        c.authority = Arc::new(TestAuthority);
    }
    let na = Arc::new(PeerNode::bind(ca).await?);
    let nb = PeerNode::bind(cb).await?;
    let address = nb.address();
    let sender = na.clone();
    let id = root.id;
    let transfer = tokio::spawn(async move { sender.sync_objects(address, &[id]).await });
    let entered =
        tokio::time::timeout(Duration::from_secs(5), blocking.started.acquire()).await??;
    entered.forget();
    assert!(
        !transfer.is_finished(),
        "success must wait for durable import"
    );
    assert!(b.export(scope(), &[id], 200_000).is_err());
    assert!(matches!(
        na.sync_objects(address, &[id]).await,
        Err(TransportError::Busy)
    ));
    let proposal = ProposalEnvelope {
        request_id: ObjectId([41; 32]),
        tenant: scope().tenant,
        domain: scope().domain,
        epoch: 1,
        branch: ObjectId([42; 32]),
        payload: ObjectId([43; 32]),
        decision: ObjectId([44; 32]),
        generation: 0,
    };
    let reply =
        tokio::time::timeout(Duration::from_secs(5), na.propose(address, proposal)).await??;
    assert_eq!(reply.status, PromotionStatus::LocalCommitted { seq: 1 });
    blocking.release();
    assert!(transfer.await??.durable);
    assert!(
        na.sync_objects(address, &[id]).await?.durable,
        "bulk capacity returns after completion"
    );
    na.close().await;
    nb.close().await;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_private_quic_transfer_acknowledges_only_durable_graph() -> Result {
    let dir = tempfile::tempdir()?;
    let a = Arc::new(SqliteObjectRepository::open(&dir.path().join("a.sqlite"))?);
    let b = Arc::new(SqliteObjectRepository::open(&dir.path().join("b.sqlite"))?);
    let root = object(Value::Bytes(vec![42; 100_000]), BTreeSet::new());
    a.import(scope(), &[root.id], vec![root.clone()], 200_000)?;
    let ka = SecretKey::from_bytes(&[11; 32]);
    let kb = SecretKey::from_bytes(&[12; 32]);
    let node_a = PeerNode::bind(config(ka.clone(), &kb, a)).await?;
    let node_b = PeerNode::bind(config(kb, &ka, b.clone())).await?;
    let report = node_a.sync_objects(node_b.address(), &[root.id]).await?;
    assert!(report.durable);
    assert_eq!(report.objects, 1);
    assert_eq!(b.export(scope(), &[root.id], 200_000)?, vec![root]);
    node_a.close().await;
    node_b.close().await;
    Ok(())
}
