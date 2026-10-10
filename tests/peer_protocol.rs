//! Real loopback integration plus a separately labelled application fault model.
//! The fence here is an owned single-process fixture, not a deployment service.
use attow_network::{
    ObjectId,
    core::persistence::{LocalStateStorage, StoredStateView},
    ledger::{
        Authority, AuthorityCheckpoint, FenceRequest, LedgerError, Policy, PromotionPayload,
        WriterFence, WriterLease,
    },
    peer::PeerAuthority,
    protocol::{PromotionStatus, ProposalEnvelope},
    state::{Worldline, fork},
    store::Store,
    transport::{
        DenyProposals, PeerConfig, PeerNode, PeerPermissions, ProposalContext, ProposalHandler,
        Scope, SecretKey, SqliteObjectRepository, TransportLimits,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    sync::{Arc, Mutex},
};
type TestResult = Result<(), Box<dyn Error>>;
const TENANT: ObjectId = ObjectId([10; 32]);
const WRITER: ObjectId = ObjectId([11; 32]);

#[derive(Default)]
struct FenceState {
    head: Option<AuthorityCheckpoint>,
    active: bool,
}
#[derive(Default, Clone)]
struct FixtureFence(Arc<Mutex<FenceState>>);
struct FixtureLease(Arc<Mutex<FenceState>>);
impl WriterFence for FixtureFence {
    fn acquire(&self, request: &FenceRequest) -> Result<Box<dyn WriterLease>, LedgerError> {
        let mut state = self.0.lock().map_err(|_| LedgerError::Fenced)?;
        if state.active
            || request.writer != WRITER
            || state.head.is_some_and(|head| head != request.checkpoint)
        {
            return Err(LedgerError::Fenced);
        }
        state.active = true;
        state.head = Some(request.checkpoint);
        Ok(Box::new(FixtureLease(self.0.clone())))
    }
}
impl WriterLease for FixtureLease {
    fn check(&self, checkpoint: &AuthorityCheckpoint) -> Result<(), LedgerError> {
        let state = self.0.lock().map_err(|_| LedgerError::Fenced)?;
        if state.active && state.head == Some(*checkpoint) {
            Ok(())
        } else {
            Err(LedgerError::Fenced)
        }
    }
    fn advance(
        &self,
        previous: &AuthorityCheckpoint,
        next: &AuthorityCheckpoint,
    ) -> Result<(), LedgerError> {
        let mut state = self.0.lock().map_err(|_| LedgerError::Fenced)?;
        if !state.active || state.head != Some(*previous) {
            return Err(LedgerError::Fenced);
        }
        state.head = Some(*next);
        Ok(())
    }
}
impl Drop for FixtureLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.active = false;
        }
    }
}

fn fixture(
    path: &std::path::Path,
) -> Result<(Authority, FixtureFence, ProposalEnvelope), Box<dyn Error>> {
    let mut authority = Authority::open(path)?;
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::new(),
        max_effect_cost: 0,
        invariants: vec![],
    })?;
    let decision = ObjectId([12; 32]);
    authority.advance_generation(decision, 0)?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: None,
    })?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [13; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    branch.put(b"selected".to_vec(), b"one".to_vec())?;
    let frozen = branch.freeze()?;
    let mut store = Store::open(path)?;
    let descriptor = frozen.admit(&mut LocalStateStorage(&mut store), "peer-test-proposal")?;
    let proposal = ProposalEnvelope {
        request_id: ObjectId([14; 32]),
        tenant: TENANT,
        domain: ObjectId(authority.domain().0),
        epoch: authority.epoch(),
        branch: descriptor,
        payload,
        decision,
        generation: 0,
    };
    SqliteObjectRepository::open(path)?.grant(
        Scope {
            tenant: TENANT,
            domain: proposal.domain,
        },
        &[proposal.branch, proposal.payload],
    )?;
    let fence = FixtureFence::default();
    authority.initialize_federation(WRITER, &fence)?;
    Ok((authority, fence, proposal))
}

#[test]
fn local_authority_does_not_implicitly_accept_remote_writer_role() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("local.sqlite");
    assert!(PeerAuthority::new(TENANT, WRITER, Authority::open(&path)?, &path).is_err());
    Ok(())
}

#[test]
fn application_fault_model_lost_reply_duplicate_and_altered_retries() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.sqlite");
    let (authority, _fence, proposal) = fixture(&path)?;
    let application = PeerAuthority::new(TENANT, WRITER, authority, &path)?;
    let context = ProposalContext {
        peer: [20; 32],
        tenant: TENANT,
        domain: proposal.domain,
    };
    let first = application.propose(context, proposal.clone())?; // Reply lost after commit.
    for _ in 0..4 {
        assert_eq!(application.propose(context, proposal.clone())?, first);
    }
    assert!(matches!(
        first.status,
        PromotionStatus::LocalCommitted { .. }
    ));
    let mut altered = proposal.clone();
    altered.generation = 1;
    assert!(application.propose(context, altered).is_err());
    let mut stale = proposal.clone();
    stale.epoch += 1;
    assert!(application.propose(context, stale).is_err());
    let mut wrong_scope = context;
    wrong_scope.tenant = ObjectId([99; 32]);
    assert!(application.propose(wrong_scope, proposal.clone()).is_err());
    // A disconnected peer can fork the old root, but its stale read cannot win.
    let mut objects = Store::open(&path)?;
    let original = attow_network::state::FrozenBranch::reopen(
        &StoredStateView(&objects),
        proposal.branch,
        attow_network::state::DomainId(proposal.domain.0),
    )?;
    let mut offline = fork(original.domain(), original.base(), [41; 32])?;
    offline.set_worldline(original.worldline().clone())?;
    assert_eq!(offline.get(b"selected")?, None);
    offline.put(b"offline".to_vec(), b"speculation".to_vec())?;
    let descriptor = offline
        .freeze()?
        .admit(&mut LocalStateStorage(&mut objects), "offline-proposal")?;
    SqliteObjectRepository::open(&path)?.grant(
        Scope {
            tenant: TENANT,
            domain: proposal.domain,
        },
        &[descriptor],
    )?;
    let conflict = application.propose(
        context,
        ProposalEnvelope {
            request_id: ObjectId([42; 32]),
            branch: descriptor,
            ..proposal
        },
    )?;
    assert!(matches!(conflict.status, PromotionStatus::Conflict { .. }));
    let inspection = Authority::inspect(&path)?;
    assert_eq!(
        inspection.current_root()?.get(b"selected")?,
        Some(b"one".to_vec())
    );
    assert_eq!(inspection.current_root()?.get(b"offline")?, None);
    Ok(())
}

#[test]
fn knowing_a_foreign_scoped_branch_or_payload_id_does_not_authorize_proposal() -> TestResult {
    for revoke_payload in [false, true] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("authority.sqlite");
        let (authority, _fence, proposal) = fixture(&path)?;
        let objects = SqliteObjectRepository::open(&path)?;
        let root = if revoke_payload {
            proposal.payload
        } else {
            proposal.branch
        };
        objects.revoke(
            Scope {
                tenant: TENANT,
                domain: proposal.domain,
            },
            &[root],
        )?;
        objects.grant(
            Scope {
                tenant: ObjectId([98; 32]),
                domain: proposal.domain,
            },
            &[root],
        )?;
        let context = ProposalContext {
            peer: [20; 32],
            tenant: TENANT,
            domain: proposal.domain,
        };
        let application = PeerAuthority::new(TENANT, WRITER, authority, &path)?;
        assert!(
            application.propose(context, proposal).is_err(),
            "foreign-scope object must not enter promotion"
        );
        assert_eq!(
            Authority::inspect(&path)?
                .current_root()?
                .get(b"selected")?,
            None
        );
    }
    Ok(())
}

#[test]
fn service_requires_its_configured_writer_identity() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.sqlite");
    let (authority, _fence, _) = fixture(&path)?;
    assert!(PeerAuthority::new(TENANT, ObjectId([99; 32]), authority, &path).is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_private_peers_commit_once_and_import_only_data() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.sqlite");
    let (authority, _fence, proposal) = fixture(&path)?;
    let domain = proposal.domain;
    let authority_objects = Arc::new(SqliteObjectRepository::open(&path)?);
    authority_objects.grant(
        Scope {
            tenant: TENANT,
            domain,
        },
        &[proposal.branch, proposal.payload],
    )?;
    let reader_path = dir.path().join("reader.sqlite");
    let reader_objects = Arc::new(SqliteObjectRepository::open(&reader_path)?);
    let a_key = SecretKey::from_bytes(&[21; 32]);
    let b_key = SecretKey::from_bytes(&[22; 32]);
    let a_id = *a_key.public().as_bytes();
    let b_id = *b_key.public().as_bytes();
    let permissions = PeerPermissions {
        objects: true,
        proposals: true,
    };
    let a = PeerNode::bind(PeerConfig {
        secret_key: a_key,
        bind_addr: "127.0.0.1:0".parse()?,
        tenant: TENANT,
        domain,
        permissions: BTreeMap::from([(b_id, permissions)]),
        limits: TransportLimits::default(),
        objects: authority_objects,
        authority: Arc::new(PeerAuthority::new(TENANT, WRITER, authority, &path)?),
    })
    .await?;
    let b = PeerNode::bind(PeerConfig {
        secret_key: b_key,
        bind_addr: "127.0.0.1:0".parse()?,
        tenant: TENANT,
        domain,
        permissions: BTreeMap::from([(a_id, permissions)]),
        limits: TransportLimits::default(),
        objects: reader_objects,
        authority: Arc::new(DenyProposals),
    })
    .await?;
    let sync = a
        .sync_objects(b.address(), &[proposal.branch, proposal.payload])
        .await?;
    assert!(sync.durable);
    assert!(sync.objects >= 3);
    let first = b.propose(a.address(), proposal.clone()).await?;
    assert_eq!(first, b.propose(a.address(), proposal.clone()).await?);
    assert!(matches!(
        first.status,
        PromotionStatus::LocalCommitted { .. }
    ));
    let reader = Store::open_read_only(&reader_path)?;
    let branch = attow_network::state::FrozenBranch::reopen(
        &StoredStateView(&reader),
        proposal.branch,
        attow_network::state::DomainId(domain.0),
    )?;
    assert_eq!(branch.object_id()?, proposal.branch);
    // Receiving a descriptor does not create authority metadata in the imported CAS.
    assert!(Authority::inspect(&reader_path).is_err());
    b.close().await;
    a.close().await;
    Ok(())
}
