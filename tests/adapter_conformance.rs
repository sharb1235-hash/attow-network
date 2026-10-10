use attow_network::{
    ObjectId,
    adapters::{
        evidence::ProviderError,
        gateway::Gateway,
        provider_simulator::{Fault, Purchase, SimulatorConfig, SimulatorServer},
    },
    ledger::{
        Authority, EffectProposal, EffectState, Policy, PromotionOutcome, PromotionPayload,
        PromotionRequest,
    },
    state::{Worldline, fork},
};
use std::collections::BTreeSet;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(feature = "browser")]
#[test]
fn actual_browser_worker_restores_owned_state_and_labels_preview_as_model() -> TestResult {
    use attow_network::{
        adapters::{
            Adapter, AdapterStore, Fidelity, ObservationRequest, WorkerBinding,
            browser::{BROWSER_BUDGET, BrowserAdapter, BrowserConfig, StorageKind},
        },
        codec::Value,
        state::execution::{EffectClass, ResourceClass, StepExit, StepRequest, WorkerBackend},
    };
    let dir = tempfile::tempdir()?;
    let store = AdapterStore::open(&dir.path().join("browser.db"))?;
    let ids = BrowserAdapter::identities(&store, "test:browser-identities")?;
    let owner = ObjectId([8; 32]);
    let binding = WorkerBinding {
        tenant: owner,
        security_scope: owner,
        policy: ids.profile,
        code: ids.code,
        profile: ids.profile,
        fidelity: ids.fidelity,
        effect_class: EffectClass::FreshObservation,
        budget: BROWSER_BUDGET,
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let node = std::env::var_os("ATTOW_NODE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join(".tools/node-v24.21.0-win-x64/node.exe"));
    let mut browser = BrowserAdapter::launch(
        store.clone(),
        binding,
        BrowserConfig::new(node, root.join("demos/workflow-studio/browser/sidecar.mjs")),
    )?;
    BrowserAdapter::manifest().validate()?;
    assert!(!browser.capabilities().isolated_remote_state);
    browser.set_storage(StorageKind::SessionStorage, "offer", "one seat")?;
    let snapshot = browser.snapshot()?;
    let continuation = BrowserAdapter::store_request(&store, "test:request", "checkout_preview")?;
    assert!(BrowserAdapter::store_request(&store, "test:request", "eval").is_err());
    let previous = store.put_object("test:observation", Value::Null, BTreeSet::new())?;
    let request = StepRequest {
        attempt: ObjectId([9; 32]),
        tenant: owner,
        security_scope: owner,
        code: ids.code,
        profile: ids.profile,
        policy: ids.profile,
        fidelity: ids.fidelity,
        effect_class: EffectClass::FreshObservation,
        resource_class: ResourceClass::Browser,
        budget: BROWSER_BUDGET,
        sample_id: None,
        input_root: snapshot,
        continuation,
        observation_root: previous,
    };
    for field in 0..11 {
        let mut changed = request.clone();
        let other = ObjectId([7; 32]);
        match field {
            0 => changed.tenant = other,
            1 => changed.security_scope = other,
            2 => changed.code = other,
            3 => changed.profile = other,
            4 => changed.policy = other,
            5 => changed.fidelity = other,
            6 => changed.effect_class = EffectClass::Pure,
            7 => changed.budget.output_bytes -= 1,
            8 => changed.resource_class = ResourceClass::Sql,
            9 => changed.sample_id = Some(other),
            _ => changed.input_root = previous,
        }
        assert!(
            browser.validate_request(&changed).is_err(),
            "binding {field}"
        );
        assert!(browser.run_step(changed).is_err());
    }
    assert!(!browser.permits_pure_reuse(&request));
    browser.set_storage(StorageKind::SessionStorage, "offer", "changed")?;
    let outcome = browser.run_step(request.clone())?;
    assert_eq!(
        browser.read_storage(StorageKind::SessionStorage, "offer")?,
        Some("one seat".into())
    );
    assert_eq!(outcome.attempt, request.attempt);
    let StepExit::Completed {
        result,
        observation_root,
        ..
    } = outcome.exit
    else {
        return Err("incomplete browser".into());
    };
    assert_eq!(result, observation_root);
    let object = store.get_object(result)?;
    assert!(object.refs.is_superset(&BTreeSet::from([
        snapshot,
        continuation,
        previous,
        ids.profile
    ])));
    let Value::Array(fields) = object.payload else {
        return Err("observation shape".into());
    };
    assert_eq!(fields[2], Value::Text("Model".into()));
    let observation = browser.observe(ObservationRequest {
        method: "checkout_preview".into(),
        input: None,
    })?;
    assert_eq!(observation.fidelity, Fidelity::Model);
    assert_eq!(observation.provenance, ids.profile);
    assert!(observation.captured_at_unix_ms > 0);
    let probe = browser.network_probe()?;
    assert_eq!(probe["forbidden_requests"], 0);
    assert_eq!(probe["service_worker_requests"], 0);
    assert_eq!(probe["fetch_blocked"], true);
    assert_eq!(probe["websocket_blocked"], true);
    assert_eq!(probe["service_worker_blocked"], true);
    Ok(())
}
fn intents(
    authority: &mut Authority,
    server: &SimulatorServer,
    count: u8,
) -> Result<(ObjectId, usize), Box<dyn std::error::Error>> {
    let contract = server.contract_id();
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("budget", 10)?;
    let decision = ObjectId([1; 32]);
    authority.advance_generation(decision, 0)?;
    let root = authority.current_root()?;
    let mut winner = None;
    let mut conflicts = 0;
    for n in 1..=count {
        let mut branch = fork(authority.domain(), &root, [n; 32])?;
        branch.set_worldline(Worldline {
            policy: Some(policy),
            ..Worldline::default()
        })?;
        let payload = authority.store_payload(&PromotionPayload {
            domain: authority.domain(),
            epoch: authority.epoch(),
            policy,
            effect: Some(EffectProposal {
                contract,
                provider: SimulatorConfig::default().provider,
                semantic_slot: "checkout".into(),
                operation: ObjectId([n; 32]),
                right: "budget".into(),
                cost: 1,
                body: Purchase { quantity: 1 }.encode()?,
            }),
        })?;
        match authority.promote(PromotionRequest {
            id: ObjectId([n + 10; 32]),
            payload,
            branch: branch.freeze()?,
            decision,
            generation: 0,
        })? {
            PromotionOutcome::AcceptedPending { intent, .. } => winner = Some(intent),
            PromotionOutcome::Conflict(_) => conflicts += 1,
            _ => return Err("unexpected local commit".into()),
        }
    }
    Ok((winner.ok_or("no intent")?, conflicts))
}
#[test]
fn five_branches_make_one_actual_send_and_response_loss_reconciles_after_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let provider_path = dir.path().join("server.sqlite");
    let ledger_path = dir.path().join("ledger.sqlite");
    let server = SimulatorServer::start(&provider_path, &SimulatorConfig::default())?;
    let mut authority = Authority::open(&ledger_path)?;
    let (intent, conflicts) = intents(&mut authority, &server, 5)?;
    assert_eq!(conflicts, 4);
    server.controller().set_fault(Fault::CommitThenDisconnect)?;
    let report = Gateway::new(&mut authority, &mut server.connect()).dispatch(intent)?;
    assert_eq!(report.state, EffectState::Unknown);
    assert_eq!(report.provider_error, Some(ProviderError::Unavailable));
    assert_eq!(server.controller().stats()?.accepted, 1);
    assert_eq!(server.controller().stats()?.inventory, 9);
    assert_eq!(authority.right_balance("budget")?.reserved, 1);
    drop(server);
    drop(authority);
    let server = SimulatorServer::start(&provider_path, &SimulatorConfig::default())?;
    let mut authority = Authority::open(&ledger_path)?;
    server.controller().set_fault(Fault::HideNextLookup)?;
    assert_eq!(
        Gateway::new(&mut authority, &mut server.connect())
            .reconcile(intent)?
            .state,
        EffectState::Unknown
    );
    assert_eq!(authority.right_balance("budget")?.reserved, 1);
    assert_eq!(
        Gateway::new(&mut authority, &mut server.connect())
            .reconcile(intent)?
            .state,
        EffectState::Confirmed
    );
    let stats = server.controller().stats()?;
    assert_eq!(stats.dispatch_requests, 1);
    assert_eq!(stats.lookups, 2);
    assert_eq!(stats.accepted, 1);
    assert_eq!(authority.right_balance("budget")?.spent, 1);
    authority.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn expired_provider_retention_never_authorizes_an_unsafe_resend() -> TestResult {
    let dir = tempfile::tempdir()?;
    let server = SimulatorServer::start(
        &dir.path().join("provider.sqlite"),
        &SimulatorConfig::default(),
    )?;
    let mut authority = Authority::open(&dir.path().join("ledger.sqlite"))?;
    let (intent, _) = intents(&mut authority, &server, 1)?;
    server.controller().set_fault(Fault::CommitThenDisconnect)?;
    Gateway::new(&mut authority, &mut server.connect()).dispatch(intent)?;
    server.controller().advance_ticks(101)?;
    let report = Gateway::new(&mut authority, &mut server.connect()).reconcile(intent)?;
    assert_eq!(report.state, EffectState::Unknown);
    assert_eq!(report.provider_error, Some(ProviderError::RetentionExpired));
    assert!(
        Gateway::new(&mut authority, &mut server.connect())
            .dispatch(intent)
            .is_err()
    );
    assert_eq!(server.controller().stats()?.dispatch_requests, 1);
    assert_eq!(authority.right_balance("budget")?.reserved, 1);
    Ok(())
}
#[test]
fn independent_providers_expose_partial_outcome_and_expired_holds_reject() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut states = Vec::new();
    for i in 0..2 {
        let server = SimulatorServer::start(
            &dir.path().join(format!("provider{i}.sqlite")),
            &SimulatorConfig::default(),
        )?;
        let mut authority = Authority::open(&dir.path().join(format!("ledger{i}.sqlite")))?;
        let (intent, _) = intents(&mut authority, &server, 1)?;
        if i == 1 {
            server.controller().hold(ObjectId([1; 32]), 1)?;
            server.controller().advance_ticks(10)?;
        }
        states.push(
            Gateway::new(&mut authority, &mut server.connect())
                .dispatch(intent)?
                .state,
        );
        assert_eq!(
            server.controller().stats()?.accepted,
            if i == 0 { 1 } else { 0 }
        );
    }
    assert_eq!(
        states,
        vec![EffectState::Confirmed, EffectState::DefinitivelyRejected]
    );
    Ok(())
}
