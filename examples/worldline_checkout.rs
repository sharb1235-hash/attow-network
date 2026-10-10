//! Five real speculative proposals compete for one durable local effect slot.
//! This example never invokes a provider or fabricates a confirmation receipt.

use std::{collections::BTreeSet, error::Error};

use chronos_codec::ObjectId;
use chronos_ledger::{
    Authority, EffectProposal, EffectState, Policy, PromotionOutcome, PromotionPayload,
    PromotionRequest,
};
use chronos_state::{Worldline, fork};

fn main() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let mut authority = Authority::open(&directory.path().join("checkout.sqlite"))?;
    let contract = ObjectId([1; 32]);
    let decision = ObjectId([2; 32]);
    let operation = ObjectId([3; 32]);
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 100,
        invariants: vec![],
    })?;
    authority.grant_right("fixture-budget", 1_000)?;
    authority.advance_generation(decision, 0)?;
    let base = authority.current_root()?;
    let mut proposals = Vec::with_capacity(5);

    println!("Attow Network: five alternatives, one local authority");
    for index in 0_u8..5 {
        let offer = format!("owned-fixture-offer-{}", index + 1);
        let cost = 20 + u64::from(index);
        let mut branch = fork(authority.domain(), &base, [index + 1; 32])?;
        branch.set_worldline(Worldline {
            policy: Some(policy),
            ..Worldline::default()
        })?;
        branch.put(
            b"selection/proposed-offer".to_vec(),
            offer.as_bytes().to_vec(),
        )?;
        let payload = authority.store_payload(&PromotionPayload {
            domain: authority.domain(),
            epoch: authority.epoch(),
            policy,
            effect: Some(EffectProposal {
                contract,
                provider: "owned-fixture".into(),
                semantic_slot: "one-booking-for-one-traveler".into(),
                operation,
                right: "fixture-budget".into(),
                cost,
                body: offer.as_bytes().to_vec(),
            }),
        })?;
        println!("predicted: {offer}, cost={cost} fixture units, no reservation");
        proposals.push(PromotionRequest {
            id: ObjectId([20 + index; 32]),
            payload,
            branch: branch.freeze()?,
            decision,
            generation: 0,
        });
    }

    let mut pending = Vec::new();
    let mut conflicts = 0;
    for proposal in proposals {
        match authority.promote(proposal)? {
            PromotionOutcome::AcceptedPending { seq, intent } => {
                println!("pending: authorization sequence={seq}, intent={intent}");
                pending.push(intent);
            }
            PromotionOutcome::Conflict(report) => {
                println!("unselected: {report:?}");
                conflicts += 1;
            }
            PromotionOutcome::LocalCommitted { .. } => {
                return Err("effect proposal unexpectedly returned a local-only result".into());
            }
        }
    }
    if pending.len() != 1 || conflicts != 4 {
        return Err("five competing proposals did not yield exactly one pending intent".into());
    }
    if authority.effect_state(pending[0])? != EffectState::Authorized {
        return Err("pending intent unexpectedly left the authorized state".into());
    }
    let verification = authority.rebuild_and_verify()?;
    println!("confirmed: 0; external provider calls: 0");
    println!("replay verified: {verification:?}");
    println!("The selected offer is locally authorized and still awaits provider evidence.");
    Ok(())
}
