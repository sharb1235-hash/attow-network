use attow_adapters::{
    evidence::{EffectProvider, ProviderError, ProviderEvidence},
    gateway::{DispatchAuthorization, Gateway},
};
use chronos_codec::ObjectId;
use chronos_ledger::{
    Authority, DispatchTicket, EffectProposal, EffectState, EvidenceStatus, EvidenceVerifier,
    LedgerError, Policy, PromotionOutcome, PromotionPayload, PromotionRequest,
};
use chronos_state::{Worldline, fork};
use std::collections::BTreeSet;
struct Provider {
    contract: ObjectId,
    calls: usize,
    queries: usize,
    lose: bool,
    bad: bool,
}
impl EvidenceVerifier for Provider {
    fn contract_id(&self) -> ObjectId {
        self.contract
    }
    fn verify(&self, _: &DispatchTicket, bytes: &[u8]) -> Result<EvidenceStatus, LedgerError> {
        match bytes {
            [] => Ok(EvidenceStatus::Unknown),
            [1] => Ok(EvidenceStatus::Confirmed),
            _ => Err(LedgerError::InvalidEvidence),
        }
    }
}
impl EffectProvider for Provider {
    fn preflight(&self, proposal: &EffectProposal) -> Result<(), ProviderError> {
        if proposal.contract == self.contract
            && proposal.provider == "fixture"
            && proposal.cost == 1
            && proposal.body == b"exact"
        {
            Ok(())
        } else {
            Err(ProviderError::InvalidAuthorization)
        }
    }
    fn dispatch(
        &mut self,
        auth: &DispatchAuthorization<'_>,
    ) -> Result<ProviderEvidence, ProviderError> {
        assert!(auth.ticket().may_dispatch());
        assert_eq!(auth.body(), b"exact");
        self.calls += 1;
        if self.lose {
            return Err(ProviderError::Unavailable);
        }
        ProviderEvidence::from_bytes(vec![if self.bad { 8 } else { 1 }])
    }
    fn reconcile(
        &mut self,
        auth: &DispatchAuthorization<'_>,
    ) -> Result<ProviderEvidence, ProviderError> {
        assert!(!auth.ticket().may_dispatch());
        assert_eq!(auth.body(), b"exact");
        self.queries += 1;
        ProviderEvidence::from_bytes(vec![1])
    }
}
fn prepare(
    authority: &mut Authority,
    contract: ObjectId,
) -> Result<ObjectId, Box<dyn std::error::Error>> {
    prepare_effect(authority, contract, "fixture", b"exact".to_vec(), 1)
}
fn prepare_effect(
    authority: &mut Authority,
    contract: ObjectId,
    provider: &str,
    body: Vec<u8>,
    cost: u64,
) -> Result<ObjectId, Box<dyn std::error::Error>> {
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("budget", 10)?;
    let decision = ObjectId([1; 32]);
    authority.advance_generation(decision, 0)?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [2; 32])?;
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
            provider: provider.into(),
            semantic_slot: "one".into(),
            operation: ObjectId([3; 32]),
            right: "budget".into(),
            cost,
            body,
        }),
    })?;
    let PromotionOutcome::AcceptedPending { intent, .. } = authority.promote(PromotionRequest {
        id: ObjectId([4; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })?
    else {
        return Err("promotion".into());
    };
    Ok(intent)
}

#[test]
fn purchase_body_must_match_reserved_cost_before_claim_or_any_provider_send()
-> Result<(), Box<dyn std::error::Error>> {
    use attow_adapters::provider_simulator::{Purchase, SimulatorConfig, SimulatorServer};
    for (body, cost) in [
        (Purchase { quantity: 2 }.encode()?, 1),
        (Purchase { quantity: 1 }.encode()?, 2),
        (b"malformed purchase".to_vec(), 1),
    ] {
        let directory = tempfile::tempdir()?;
        let config = SimulatorConfig::default();
        let server = SimulatorServer::start(&directory.path().join("provider.sqlite"), &config)?;
        let mut authority = Authority::open(&directory.path().join("ledger.sqlite"))?;
        let intent = prepare_effect(
            &mut authority,
            server.contract_id(),
            &config.provider,
            body,
            cost,
        )?;
        let before = authority.rebuild_and_verify()?;
        let mut provider = server.connect();
        assert!(
            Gateway::new(&mut authority, &mut provider)
                .dispatch(intent)
                .is_err(),
            "mismatched body/cost reached provider"
        );
        assert_eq!(authority.effect_state(intent)?, EffectState::Authorized);
        assert_eq!(authority.right_balance("budget")?.reserved, cost);
        assert_eq!(authority.right_balance("budget")?.spent, 0);
        assert_eq!(authority.rebuild_and_verify()?, before);
        let stats = server.controller().stats()?;
        assert_eq!(stats.dispatch_requests, 0);
        assert_eq!(stats.accepted, 0);
        assert_eq!(stats.inventory, config.inventory);
    }
    Ok(())
}
#[test]
fn response_loss_is_unknown_reconciliation_uses_same_intent_without_resend()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut authority = Authority::open(&dir.path().join("ledger.sqlite"))?;
    let contract = ObjectId([8; 32]);
    let intent = prepare(&mut authority, contract)?;
    let mut provider = Provider {
        contract,
        calls: 0,
        queries: 0,
        lose: true,
        bad: false,
    };
    let report = Gateway::new(&mut authority, &mut provider).dispatch(intent)?;
    assert_eq!(report.state, EffectState::Unknown);
    assert_eq!(report.provider_error, Some(ProviderError::Unavailable));
    assert_eq!(authority.latest_receipt(intent)?, Some(report.receipt));
    assert_eq!(authority.right_balance("budget")?.reserved, 1);
    assert!(
        Gateway::new(&mut authority, &mut provider)
            .dispatch(intent)
            .is_err()
    );
    assert_eq!(provider.calls, 1);
    assert_eq!(
        Gateway::new(&mut authority, &mut provider)
            .reconcile(intent)?
            .state,
        EffectState::Confirmed
    );
    assert_eq!(provider.calls, 1);
    assert_eq!(provider.queries, 1);
    assert_eq!(authority.right_balance("budget")?.spent, 1);
    authority.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn wrong_contract_never_claims_and_invalid_receipt_never_confirms()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut authority = Authority::open(&dir.path().join("ledger.sqlite"))?;
    let contract = ObjectId([8; 32]);
    let intent = prepare(&mut authority, contract)?;
    let mut provider = Provider {
        contract: ObjectId([9; 32]),
        calls: 0,
        queries: 0,
        lose: false,
        bad: true,
    };
    assert!(
        Gateway::new(&mut authority, &mut provider)
            .dispatch(intent)
            .is_err()
    );
    assert_eq!(authority.effect_state(intent)?, EffectState::Authorized);
    assert_eq!(provider.calls, 0);
    provider.contract = contract;
    let report = Gateway::new(&mut authority, &mut provider).dispatch(intent)?;
    assert_eq!(report.state, EffectState::Unknown);
    assert_eq!(report.provider_error, Some(ProviderError::InvalidEvidence));
    assert_eq!(authority.right_balance("budget")?.reserved, 1);
    Ok(())
}
#[test]
fn provider_without_explicit_preflight_cannot_acquire_dispatch_authorization()
-> Result<(), Box<dyn std::error::Error>> {
    struct Unadmitted(Provider);
    impl EvidenceVerifier for Unadmitted {
        fn contract_id(&self) -> ObjectId {
            self.0.contract
        }
        fn verify(
            &self,
            ticket: &DispatchTicket,
            receipt: &[u8],
        ) -> Result<EvidenceStatus, LedgerError> {
            self.0.verify(ticket, receipt)
        }
    }
    impl EffectProvider for Unadmitted {
        fn dispatch(
            &mut self,
            auth: &DispatchAuthorization<'_>,
        ) -> Result<ProviderEvidence, ProviderError> {
            self.0.dispatch(auth)
        }
        fn reconcile(
            &mut self,
            auth: &DispatchAuthorization<'_>,
        ) -> Result<ProviderEvidence, ProviderError> {
            self.0.reconcile(auth)
        }
    }
    let directory = tempfile::tempdir()?;
    let mut authority = Authority::open(&directory.path().join("ledger.sqlite"))?;
    let contract = ObjectId([8; 32]);
    let intent = prepare(&mut authority, contract)?;
    let mut provider = Unadmitted(Provider {
        contract,
        calls: 0,
        queries: 0,
        lose: false,
        bad: false,
    });
    assert!(
        Gateway::new(&mut authority, &mut provider)
            .dispatch(intent)
            .is_err()
    );
    assert_eq!(authority.effect_state(intent)?, EffectState::Authorized);
    assert_eq!(provider.0.calls, 0);
    Ok(())
}
