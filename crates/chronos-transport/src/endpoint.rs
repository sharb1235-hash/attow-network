use crate::{
    ObjectRepository, PeerAddress, PeerConfig, PeerPermissions, ProposalHandler, Scope, SyncReport,
    TransportError, TransportLimits,
};
use chronos_codec::ObjectId;
use chronos_protocol::{ALPN, PromotionReply, ProposalEnvelope};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMode,
    endpoint::{Connection, QuicTransportConfig, presets},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task::{JoinHandle, JoinSet},
};

pub(crate) struct Slots {
    control: Arc<Semaphore>,
    bulk: Arc<Semaphore>,
}
pub(crate) struct Inner {
    pub scope: Scope,
    pub limits: TransportLimits,
    pub objects: Arc<dyn ObjectRepository>,
    pub authority: Arc<dyn ProposalHandler>,
    pub permissions: BTreeMap<[u8; 32], PeerPermissions>,
    peer_slots: BTreeMap<[u8; 32], Slots>,
    slots: Slots,
    control_bytes: Arc<Semaphore>,
    bulk_bytes: Arc<Semaphore>,
}
pub(crate) struct Lease {
    _global: OwnedSemaphorePermit,
    _peer: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}
impl Inner {
    pub fn duration(&self) -> Duration {
        Duration::from_millis(self.limits.deadline_ms)
    }
    pub fn lease(&self, peer: [u8; 32], bulk: bool) -> Result<Lease, TransportError> {
        let p = self
            .peer_slots
            .get(&peer)
            .ok_or(TransportError::Unauthorized)?;
        let (global, per_peer, bytes, count) = if bulk {
            (
                &self.slots.bulk,
                &p.bulk,
                &self.bulk_bytes,
                self.limits.max_batch_bytes,
            )
        } else {
            (
                &self.slots.control,
                &p.control,
                &self.control_bytes,
                self.limits.protocol.control_bytes,
            )
        };
        Ok(Lease {
            _global: global
                .clone()
                .try_acquire_owned()
                .map_err(|_| TransportError::Busy)?,
            _peer: per_peer
                .clone()
                .try_acquire_owned()
                .map_err(|_| TransportError::Busy)?,
            _bytes: bytes
                .clone()
                .try_acquire_many_owned(count as u32)
                .map_err(|_| TransportError::Busy)?,
        })
    }
}
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private(),
        IpAddr::V6(v) => v.is_loopback() || v.is_unique_local(),
    }
}
impl TransportLimits {
    pub fn validate(&self) -> Result<(), TransportError> {
        self.protocol.validate()?;
        if self.protocol.control_bytes < 256
            || self.max_batch_bytes == 0
            || self.max_batch_bytes > 64 * 1024 * 1024
            || self.bulk_bytes < self.max_batch_bytes
            || self.bulk_bytes > 256 * 1024 * 1024
            || self.control_bytes < self.protocol.control_bytes
            || self.control_bytes > 32 * 1024 * 1024
            || self.bulk_streams == 0
            || self.control_streams == 0
            || self
                .bulk_streams
                .checked_add(self.control_streams)
                .is_none_or(|sum| sum > 32)
            || self.deadline_ms == 0
            || self.deadline_ms > 30_000
        {
            return Err(TransportError::Configuration);
        }
        Ok(())
    }
}
pub struct PeerNode {
    endpoint: Endpoint,
    inner: Arc<Inner>,
    address: PeerAddress,
    task: Mutex<Option<JoinHandle<()>>>,
}
impl PeerNode {
    pub async fn bind(config: PeerConfig) -> Result<Self, TransportError> {
        config.limits.validate()?;
        if !private(config.bind_addr.ip()) || config.permissions.len() > 64 {
            return Err(TransportError::Configuration);
        }
        let quic = QuicTransportConfig::builder()
            .max_concurrent_bidi_streams(32u32.into())
            .max_concurrent_uni_streams(0u32.into())
            .stream_receive_window(262_144u32.into())
            .receive_window(2_097_152u32.into())
            .send_window(2_097_152)
            .build();
        // Minimal does not install discovery. Clear BOTH default IP families.
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(config.secret_key)
            .clear_ip_transports()
            .bind_addr(config.bind_addr)
            .map_err(|_| TransportError::Configuration)?
            .relay_mode(RelayMode::Disabled)
            .alpns(vec![ALPN.to_vec()])
            .transport_config(quic)
            .bind()
            .await
            .map_err(|_| TransportError::Unavailable)?;
        let address = endpoint
            .addr()
            .ip_addrs()
            .find(|a| a.ip() == config.bind_addr.ip())
            .copied()
            .ok_or(TransportError::Unavailable)?;
        let peer = *endpoint.id().as_bytes();
        fn slots(l: &TransportLimits) -> Slots {
            Slots {
                control: Arc::new(Semaphore::new(l.control_streams)),
                bulk: Arc::new(Semaphore::new(l.bulk_streams)),
            }
        }
        let inner = Arc::new(Inner {
            scope: Scope {
                tenant: config.tenant,
                domain: config.domain,
            },
            peer_slots: config
                .permissions
                .keys()
                .map(|id| (*id, slots(&config.limits)))
                .collect(),
            slots: slots(&config.limits),
            control_bytes: Arc::new(Semaphore::new(config.limits.control_bytes)),
            bulk_bytes: Arc::new(Semaphore::new(config.limits.bulk_bytes)),
            permissions: config.permissions,
            limits: config.limits,
            objects: config.objects,
            authority: config.authority,
        });
        let server = endpoint.clone();
        let context = inner.clone();
        let task = tokio::spawn(async move {
            let mut jobs = JoinSet::new();
            loop {
                tokio::select! {
                    incoming=server.accept()=>{
                        let Some(incoming)=incoming else{break};
                        if jobs.len()>=32{incoming.refuse();continue}
                        let context=context.clone();
                        jobs.spawn(async move{
                            let _=tokio::time::timeout(context.duration(),async{
                                // Await the complete handshake. Never opt into Iroh 0-RTT.
                                let connection=incoming.await.map_err(|_|TransportError::Unavailable)?;
                                let peer=*connection.remote_id().as_bytes();
                                if connection.alpn()!=ALPN||!context.permissions.contains_key(&peer){connection.close(1u32.into(),b"denied");return Err(TransportError::Unauthorized)}
                                crate::session::serve(context,connection,peer).await
                            }).await;
                        });
                    }
                    _=jobs.join_next(),if !jobs.is_empty()=>{}
                }
            }
            jobs.abort_all();
            while jobs.join_next().await.is_some() {}
        });
        Ok(Self {
            endpoint,
            inner,
            address: PeerAddress { peer, address },
            task: Mutex::new(Some(task)),
        })
    }
    pub fn address(&self) -> PeerAddress {
        self.address
    }
    async fn connect(&self, peer: PeerAddress) -> Result<Connection, TransportError> {
        if !private(peer.address.ip()) || peer.address.port() == 0 {
            return Err(TransportError::Configuration);
        }
        if !self.inner.permissions.contains_key(&peer.peer) {
            return Err(TransportError::Unauthorized);
        }
        let id = EndpointId::from_bytes(&peer.peer).map_err(|_| TransportError::Configuration)?;
        self.endpoint
            .connect(EndpointAddr::new(id).with_ip_addr(peer.address), ALPN)
            .await
            .map_err(|_| TransportError::Unavailable)
    }
    pub async fn sync_objects(
        &self,
        peer: PeerAddress,
        roots: &[ObjectId],
    ) -> Result<SyncReport, TransportError> {
        if roots.is_empty()
            || roots.len() > self.inner.limits.protocol.max_ids
            || roots.iter().collect::<BTreeSet<_>>().len() != roots.len()
        {
            return Err(TransportError::Limit);
        }
        if !self
            .inner
            .permissions
            .get(&peer.peer)
            .is_some_and(|p| p.objects)
        {
            return Err(TransportError::Unauthorized);
        }
        let lease = self.inner.lease(peer.peer, true)?;
        let inner = self.inner.clone();
        let roots = roots.to_vec();
        tokio::time::timeout(inner.duration(), async {
            let connection = self.connect(peer).await?;
            crate::session::push(inner, connection, roots, lease).await
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }
    pub async fn propose(
        &self,
        peer: PeerAddress,
        proposal: ProposalEnvelope,
    ) -> Result<PromotionReply, TransportError> {
        if !self
            .inner
            .permissions
            .get(&peer.peer)
            .is_some_and(|p| p.proposals)
            || proposal.tenant != self.inner.scope.tenant
            || proposal.domain != self.inner.scope.domain
        {
            return Err(TransportError::Unauthorized);
        }
        let lease = self.inner.lease(peer.peer, false)?;
        let inner = self.inner.clone();
        tokio::time::timeout(inner.duration(), async {
            let connection = self.connect(peer).await?;
            crate::session::propose(inner, connection, proposal, lease).await
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }
    pub async fn close(&self) {
        self.endpoint.close().await;
        let task = self.task.lock().ok().and_then(|mut t| t.take());
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}
impl Drop for PeerNode {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}
