//! Original owned examples using actual workers and private QUIC peers.
mod graph;
mod receipt;
use crate::{CliError, records::RunRecord, workspace::Workspace};
use chronos_adapters::{
    gateway::Gateway,
    provider_simulator::{Fault, Purchase, SimulatorConfig, SimulatorServer},
};
use chronos_codec::{ObjectId, Value};
use chronos_ledger::{
    Authority, EffectProposal, EffectState, LedgerError, Policy, PromotionPayload,
};
use chronos_local::{owned_loopback::OwnedLoopbackFence, peer::PeerAuthority};
use chronos_protocol::{PromotionStatus, ProposalEnvelope};
use chronos_transport::{
    DenyProposals, ObjectRepository, PeerConfig, PeerNode, PeerPermissions, Scope, SecretKey,
    SqliteObjectRepository, TransportLimits,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Checkout,
    DataChange,
}
impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Self::Checkout => "checkout",
            Self::DataChange => "data-change",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoBoundary {
    AfterPrepared,
    AfterIntentSaved,
    AfterGatewayReturned,
    AfterResponsePrepared,
    AfterResponseCommitted,
}
pub struct DemoOptions {
    pub scenario: Scenario,
    pub workers: usize,
    pub resume: Option<String>,
    pub browser: bool,
    pub pause_after_prepared: bool,
    pub pause_after_unknown: bool,
    pub boundary_hook: Option<fn(DemoBoundary)>,
}
/// The canonical profile used by both the preview graph and receipt rerun.
pub fn execution_profile() -> Result<chronos_sandbox::ExecutionProfile, CliError> {
    Ok(chronos_sandbox::ExecutionProfile::new(
        50_000, 65_536, 1024,
    )?)
}
pub async fn run(ws: &Workspace, options: DemoOptions) -> Result<RunRecord, CliError> {
    if options.pause_after_prepared && options.pause_after_unknown {
        return Err(CliError::input("choose one pause boundary"));
    }
    if !(1..=8).contains(&options.workers) {
        return Err(CliError::input("workers must be between 1 and 8"));
    }
    #[cfg(not(feature = "browser"))]
    if options.browser {
        return Err(CliError::Infrastructure(
            "this installation has no browser feature".into(),
        ));
    }
    let mut record = if let Some(id) = &options.resume {
        if !ws.is_native(id)? {
            return Err(CliError::input("imported data cannot be resumed"));
        }
        let record = ws.load_run(id)?;
        if record.scenario != options.scenario.name()
            || record.fidelity.contains_key("browser") != options.browser
        {
            return Err(CliError::input(
                "resume must preserve the scenario and browser mode",
            ));
        }
        record
    } else {
        let mut random = [0; 32];
        getrandom::fill(&mut random).map_err(CliError::infrastructure)?;
        let mut record =
            RunRecord::new(ObjectId(random).to_string(), options.scenario.name().into());
        if options.browser {
            record
                .fidelity
                .insert("browser".into(), "OwnedEnvironment".into());
        }
        ws.save_run(&record)?;
        record
    };
    if matches!(record.phase.as_str(), "completed" | "rejected") {
        return Ok(record);
    }
    let writer = ws.writer_id()?;
    let fence = OwnedLoopbackFence::open(&ws.fence_path(), writer)?;
    let mut authority = Authority::open_federated(&ws.authority_path(), writer, &fence)?;
    let server = if options.scenario == Scenario::Checkout {
        Some(
            SimulatorServer::start(&ws.provider_path(), &SimulatorConfig::default())
                .map_err(CliError::infrastructure)?,
        )
    } else {
        None
    };
    if record.phase == "created" {
        let initial = authority.current_root()?;
        if record.initial_root.is_some()
            && required(&record.initial_root)? != initial.object_id()?
        {
            return Err(CliError::Conflict(
                "authority changed before the run was prepared".into(),
            ));
        }
        record.initial_root = Some(initial.object_id()?.to_string());
        record.managed_root = record.initial_root.clone();
        let policy = if let Some(policy) = &record.policy {
            crate::workspace::validate_id(policy)?
        } else {
            authority.configure_policy(Policy {
                allowed_contracts: server
                    .as_ref()
                    .map(|s| BTreeSet::from([s.contract_id()]))
                    .unwrap_or_default(),
                max_effect_cost: 1,
                invariants: vec![],
            })?
        };
        record.policy = Some(policy.to_string());
        let decision = derive(&record.run_id, "decision");
        ensure_generation(&mut authority, decision)?;
        record.decision = Some(decision.to_string());
        record.request_id = Some(derive(&record.run_id, "request").to_string());
        let effect = if let Some(server) = &server {
            let right = format!("demo:{}", record.run_id);
            match authority.right_balance(&right) {
                Ok(_) => {}
                Err(LedgerError::Missing) => authority.grant_right(&right, 1)?,
                Err(e) => return Err(e.into()),
            }
            let operation = derive(&record.run_id, "operation");
            record.operation = Some(operation.to_string());
            Some(EffectProposal {
                contract: server.contract_id(),
                provider: SimulatorConfig::default().provider,
                semantic_slot: format!("checkout:{}", record.run_id),
                operation,
                right,
                cost: 1,
                body: Purchase { quantity: 1 }
                    .encode()
                    .map_err(CliError::infrastructure)?,
            })
        } else {
            None
        };
        record.payload = Some(
            authority
                .store_payload(&PromotionPayload {
                    domain: authority.domain(),
                    epoch: authority.epoch(),
                    policy,
                    effect,
                })?
                .to_string(),
        );
        ws.save_run(&record)?;
        graph::prepare(ws, &mut record, &authority, &options).await?;
        record.phase = "prepared".into();
        ws.save_run(&record)?;
        boundary(&options, DemoBoundary::AfterPrepared);
    }
    if options.pause_after_prepared && record.phase == "prepared" {
        return Ok(record);
    }
    // Reusing the persisted IDs also covers a lost promotion reply.
    if record.phase == "prepared" {
        if authority
            .promotion_record(required(&record.request_id)?)?
            .is_none()
            && authority.current_root()?.object_id()? != required(&record.initial_root)?
        {
            return Err(CliError::Conflict(
                "managed state changed after this demo was prepared; prepare a new workflow".into(),
            ));
        }
        let (returned, status) = promote_over_peers(ws, &mut record, authority).await?;
        authority = returned;
        match status {
            PromotionStatus::LocalCommitted { seq } => {
                record.phase = "completed".into();
                event(
                    &mut record,
                    serde_json::json!({"kind":"local_committed","sequence":seq}),
                );
            }
            PromotionStatus::AcceptedPending { seq, intent } => {
                record.intent = Some(intent.to_string());
                record.phase = "authorized".into();
                event(
                    &mut record,
                    serde_json::json!({"kind":"effect_authorized","sequence":seq,"intent":intent.to_string()}),
                );
            }
            PromotionStatus::Conflict { .. } => {
                ws.save_run(&record)?;
                return Err(CliError::Conflict(
                    "selected proposal conflicts with the authority".into(),
                ));
            }
        }
        let committed = authority
            .promotion_record(required(&record.request_id)?)?
            .ok_or_else(|| CliError::infrastructure("missing promotion history"))?;
        let expected = crate::workspace::validate_id(&record.branches[0].state_root)?;
        if committed.state_root != expected {
            return Err(CliError::infrastructure(
                "unexpected historical managed root",
            ));
        }
        record.managed_root = Some(committed.state_root.to_string());
        ws.save_run(&record)?;
        boundary(&options, DemoBoundary::AfterIntentSaved);
    }
    if let Some(server) = &server {
        finish_effect(ws, &mut record, &mut authority, server, &options).await?;
    }
    authority.rebuild_and_verify()?;
    ws.save_run(&record)?;
    Ok(record)
}

async fn finish_effect(
    ws: &Workspace,
    record: &mut RunRecord,
    authority: &mut Authority,
    server: &SimulatorServer,
    options: &DemoOptions,
) -> Result<(), CliError> {
    let intent = required(&record.intent)?;
    let mut client = server.connect();
    let current = authority.effect_state(intent)?;
    let result = match current {
        EffectState::Authorized => {
            server
                .controller()
                .set_fault(Fault::CommitThenDisconnect)
                .map_err(CliError::infrastructure)?;
            Some(
                Gateway::new(authority, &mut client)
                    .dispatch(intent)
                    .map_err(CliError::infrastructure)?,
            )
        }
        EffectState::Unknown => Some(
            Gateway::new(authority, &mut client)
                .reconcile(intent)
                .map_err(CliError::infrastructure)?,
        ),
        EffectState::Confirmed
        | EffectState::DefinitivelyRejected
        | EffectState::CancelledBeforeSend => None,
        EffectState::Dispatching => {
            return Err(CliError::infrastructure(
                "authority recovery did not finish",
            ));
        }
    };
    if let Some(result) = result {
        boundary(options, DemoBoundary::AfterGatewayReturned);
        record.receipt = Some(result.receipt.to_string());
        event(
            record,
            serde_json::json!({"kind":if current==EffectState::Authorized{"provider_dispatch"}else{"provider_reconciliation"},"state":format!("{:?}",result.state),"receipt":result.receipt.to_string()}),
        );
    } else {
        record.receipt = authority.latest_receipt(intent)?.map(|id| id.to_string());
    }
    record.summary["provider"] = serde_json::to_value(
        server
            .controller()
            .stats()
            .map_err(CliError::infrastructure)?,
    )?;
    if authority.effect_state(intent)? == EffectState::Unknown {
        record.phase = "unknown".into();
        ws.save_run(record)?;
        if options.pause_after_unknown || current == EffectState::Unknown {
            return Ok(());
        }
        let result = Gateway::new(authority, &mut client)
            .reconcile(intent)
            .map_err(CliError::infrastructure)?;
        record.receipt = Some(result.receipt.to_string());
        event(
            record,
            serde_json::json!({"kind":"provider_reconciliation","state":format!("{:?}",result.state),"receipt":result.receipt.to_string()}),
        );
        record.summary["provider"] = serde_json::to_value(
            server
                .controller()
                .stats()
                .map_err(CliError::infrastructure)?,
        )?;
        if result.state == EffectState::Unknown {
            ws.save_run(record)?;
            return Ok(());
        }
    }
    match authority.effect_state(intent)? {
        EffectState::Confirmed => {}
        EffectState::DefinitivelyRejected | EffectState::CancelledBeforeSend => {
            record.phase = "rejected".into();
            ws.save_run(record)?;
            return Ok(());
        }
        _ => {
            return Err(CliError::infrastructure(
                "unexpected post-gateway effect state",
            ));
        }
    }
    if record.phase != "response_prepared" {
        record.phase = "confirmed".into();
        ws.save_run(record)?;
    }
    receipt::complete(ws, record, authority, options).await
}
fn boundary(options: &DemoOptions, point: DemoBoundary) {
    if let Some(hook) = options.boundary_hook {
        hook(point);
    }
}
fn derive(run: &str, label: &str) -> ObjectId {
    let mut h = blake3::Hasher::new_derive_key("attow-owned-demo-v1");
    h.update(run.as_bytes());
    h.update(&[0]);
    h.update(label.as_bytes());
    ObjectId(*h.finalize().as_bytes())
}
fn required(value: &Option<String>) -> Result<ObjectId, CliError> {
    crate::workspace::validate_id(
        value
            .as_deref()
            .ok_or_else(|| CliError::input("run lacks a durable identity"))?,
    )
}
fn worker_path(ws: &Workspace, run: &str) -> PathBuf {
    ws.data_dir().join(format!("worker-{run}.sqlite"))
}
fn event(record: &mut RunRecord, value: serde_json::Value) {
    record.events.push(value);
}
fn ensure_generation(authority: &mut Authority, id: ObjectId) -> Result<(), CliError> {
    match authority.decision_generation(id)? {
        None => authority.advance_generation(id, 0)?,
        Some(0) => {}
        Some(_) => {
            return Err(CliError::Conflict(
                "demo generation is no longer current".into(),
            ));
        }
    }
    Ok(())
}
fn save_trace(
    path: &Path,
    owner: &str,
    trace: &chronos_scheduler::ExecutionTrace,
) -> Result<ObjectId, CliError> {
    Ok(chronos_local::scheduler::SqliteResultRepository::open(path)?.store_trace(owner, trace)?)
}

async fn promote_over_peers(
    ws: &Workspace,
    record: &mut RunRecord,
    authority: Authority,
) -> Result<(Authority, PromotionStatus), CliError> {
    let tenant = derive(&record.run_id, "tenant");
    let domain = ObjectId(authority.domain().0);
    let epoch = authority.epoch();
    let scope = Scope { tenant, domain };
    let source = Arc::new(SqliteObjectRepository::open(&worker_path(
        ws,
        &record.run_id,
    ))?);
    let destination = Arc::new(SqliteObjectRepository::open(&ws.authority_path())?);
    let mut roots = BTreeSet::from([required(&record.payload)?]);
    for branch in &record.branches {
        roots.insert(crate::workspace::validate_id(&branch.descriptor)?);
        roots.insert(crate::workspace::validate_id(&branch.state_root)?);
    }
    for trace in &record.trace_roots {
        roots.insert(crate::workspace::validate_id(trace)?);
    }
    let roots = roots.into_iter().collect::<Vec<_>>();
    source.grant(scope, &roots)?;
    let bridge = Arc::new(PeerAuthority::new(
        tenant,
        ws.writer_id()?,
        authority,
        &ws.authority_path(),
    )?);
    let mut a = [0; 32];
    let mut b = [0; 32];
    getrandom::fill(&mut a).map_err(CliError::infrastructure)?;
    getrandom::fill(&mut b).map_err(CliError::infrastructure)?;
    let a = SecretKey::from_bytes(&a);
    let b = SecretKey::from_bytes(&b);
    let aid = *a.public().as_bytes();
    let bid = *b.public().as_bytes();
    let permissions = PeerPermissions {
        objects: true,
        proposals: true,
    };
    let first = PeerNode::bind(PeerConfig {
        secret_key: a,
        bind_addr: "127.0.0.1:0".parse().map_err(CliError::infrastructure)?,
        tenant,
        domain,
        permissions: BTreeMap::from([(bid, permissions)]),
        limits: TransportLimits::default(),
        objects: source,
        authority: Arc::new(DenyProposals),
    })
    .await?;
    let second = PeerNode::bind(PeerConfig {
        secret_key: b,
        bind_addr: "127.0.0.1:0".parse().map_err(CliError::infrastructure)?,
        tenant,
        domain,
        permissions: BTreeMap::from([(aid, permissions)]),
        limits: TransportLimits::default(),
        objects: destination,
        authority: bridge.clone(),
    })
    .await?;
    let result=async{
        let synced=first.sync_objects(second.address(),&roots).await?;
        event(record,serde_json::json!({"kind":"peer_transfer","durable":synced.durable,"objects":synced.objects,"bytes":synced.bytes,"roots":roots.len()}));
        let branch=record.branches.first().ok_or(chronos_transport::TransportError::InvalidGraph)?.descriptor.parse().map_err(|_|chronos_transport::TransportError::InvalidGraph)?;
        let reply=first.propose(second.address(),ProposalEnvelope{request_id:required(&record.request_id).map_err(|_|chronos_transport::TransportError::InvalidGraph)?,tenant,domain,epoch,branch,payload:required(&record.payload).map_err(|_|chronos_transport::TransportError::InvalidGraph)?,decision:required(&record.decision).map_err(|_|chronos_transport::TransportError::InvalidGraph)?,generation:record.generation}).await?;
        Ok::<_,chronos_transport::TransportError>(reply.status)
    }.await;
    first.close().await;
    second.close().await;
    drop(first);
    drop(second);
    let authority = Arc::try_unwrap(bridge)
        .map_err(|_| CliError::infrastructure("peer callback is still borrowed"))?
        .into_authority()?;
    Ok((authority, result?))
}
