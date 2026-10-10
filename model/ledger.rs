//! Independent finite model. This is not the production ledger implementation.

use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rules {
    Correct,
    SendBeforeCommit,
    ReleaseUnknown,
    DispatchDuringReplay,
    DuplicateInflatesRights,
}

#[derive(Debug, Default)]
struct Exploration {
    states: usize,
    transitions: usize,
    duplicate_edges: usize,
    lost_ack_edges: usize,
    unknown_states: usize,
    crash_states: usize,
    replay_states: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Effect {
    Absent,
    Authorized,
    Dispatching,
    Unknown,
    Confirmed,
    Rejected,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Provider {
    NotObserved,
    Performed,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reply {
    None,
    Confirmed,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Ready,
    Down,
    Recovering,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pending {
    Promote(usize),
    Claim(usize),
    Confirm(usize),
    Reject(usize),
    Unknown(usize),
    Cancel(usize),
    Recover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    mode: Mode,
    pending: Option<Pending>,
    effects: [Effect; 2],
    committed_requests: [bool; 2],
    managed_writes: [bool; 2],
    slot: Option<usize>,
    available: u8,
    reserved: [bool; 2],
    spent: [bool; 2],
    tickets: [bool; 2],
    replies: [Reply; 2],
    sends: [u8; 2],
    provider: [Provider; 2],
    crashes: u8,
    replays: u8,
    replay_provider_calls: u8,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::Ready,
            pending: None,
            effects: [Effect::Absent; 2],
            committed_requests: [false; 2],
            managed_writes: [false; 2],
            slot: None,
            available: 1,
            reserved: [false; 2],
            spent: [false; 2],
            tickets: [false; 2],
            replies: [Reply::None; 2],
            sends: [0; 2],
            provider: [Provider::NotObserved; 2],
            crashes: 0,
            replays: 0,
            replay_provider_calls: 0,
        }
    }
}

fn invariant(state: &State) -> Result<(), String> {
    let conserved = usize::from(state.available)
        + state.reserved.iter().filter(|value| **value).count()
        + state.spent.iter().filter(|value| **value).count();
    if conserved != 1 {
        return Err("the initial single right is not conserved".into());
    }
    let occupied = state
        .effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                Effect::Authorized | Effect::Dispatching | Effect::Unknown | Effect::Confirmed
            )
        })
        .count();
    if occupied > 1 {
        return Err("multiple operations occupy the same semantic slot".into());
    }
    if state.replay_provider_calls != 0 {
        return Err("replay called an external provider".into());
    }
    if state.managed_writes != state.committed_requests {
        return Err("managed state and request outcome committed separately".into());
    }
    for index in 0..2 {
        if state.sends[index] > 0 && !state.committed_requests[index] {
            return Err("send preceded durable authorization".into());
        }
        if state.tickets[index]
            && (state.effects[index] != Effect::Dispatching || !state.committed_requests[index])
        {
            return Err("a dispatch ticket has no durable dispatching record".into());
        }
        let unresolved = matches!(
            state.effects[index],
            Effect::Authorized | Effect::Dispatching | Effect::Unknown
        );
        if unresolved && (state.slot != Some(index) || !state.reserved[index]) {
            return Err("unresolved operation lost its slot or reserved right".into());
        }
        if state.effects[index] == Effect::Confirmed
            && (state.slot != Some(index) || !state.spent[index])
        {
            return Err("confirmed operation lost its occupied slot or spent right".into());
        }
        if state.provider[index] == Provider::Performed
            && !(state.reserved[index] || state.spent[index])
        {
            return Err("provider performed an effect without a retained or spent right".into());
        }
        if state.committed_requests[index] != (state.effects[index] != Effect::Absent) {
            return Err("outbox and deduplicated request split their durability boundary".into());
        }
    }
    Ok(())
}

fn clear_reservation(state: &mut State, index: usize) {
    state.reserved[index] = false;
    state.available += 1;
    state.slot = None;
}

fn transitions(state: State, rules: Rules) -> Vec<(&'static str, State)> {
    let mut edges = Vec::new();
    if state.mode != Mode::Down && state.crashes < 2 {
        let mut next = state;
        next.mode = Mode::Down;
        next.pending = None;
        next.tickets = [false; 2];
        next.replies = [Reply::None; 2];
        next.crashes += 1;
        edges.push((
            if state.pending.is_some() {
                "crash_before_commit"
            } else {
                "crash_after_commit"
            },
            next,
        ));
    }
    if state.mode == Mode::Down {
        let mut next = state;
        next.mode = Mode::Recovering;
        next.pending = Some(Pending::Recover);
        edges.push(("start_recovery", next));
        return edges;
    }
    if let Some(pending) = state.pending {
        let mut next = state;
        next.pending = None;
        match pending {
            Pending::Promote(index) => {
                next.effects[index] = Effect::Authorized;
                next.committed_requests[index] = true;
                next.managed_writes[index] = true;
                next.available -= 1;
                next.reserved[index] = true;
                next.slot = Some(index);
                if rules == Rules::SendBeforeCommit {
                    let mut premature = state;
                    premature.sends[index] = 1;
                    edges.push(("incorrect_send_before_commit", premature));
                }
            }
            Pending::Claim(index) => {
                next.effects[index] = Effect::Dispatching;
                next.tickets[index] = true;
            }
            Pending::Confirm(index) => {
                next.effects[index] = Effect::Confirmed;
                next.reserved[index] = false;
                next.spent[index] = true;
                next.tickets[index] = false;
                next.replies[index] = Reply::None;
            }
            Pending::Reject(index) | Pending::Cancel(index) => {
                next.effects[index] = if matches!(pending, Pending::Reject(_)) {
                    Effect::Rejected
                } else {
                    Effect::Cancelled
                };
                clear_reservation(&mut next, index);
                next.tickets[index] = false;
                next.replies[index] = Reply::None;
            }
            Pending::Unknown(index) => {
                next.effects[index] = Effect::Unknown;
                next.tickets[index] = false;
                next.replies[index] = Reply::None;
                if rules == Rules::ReleaseUnknown {
                    clear_reservation(&mut next, index);
                }
            }
            Pending::Recover => {
                for index in 0..2 {
                    if next.effects[index] == Effect::Dispatching {
                        next.effects[index] = Effect::Unknown;
                        if rules == Rules::ReleaseUnknown {
                            clear_reservation(&mut next, index);
                        }
                    }
                }
                next.mode = Mode::Ready;
            }
        }
        edges.push(("commit", next));
        return edges;
    }
    if state.mode != Mode::Ready {
        return edges;
    }
    if state.replays == 0 {
        let mut next = state;
        next.replays = 1;
        if rules == Rules::DispatchDuringReplay {
            next.replay_provider_calls = 1;
        }
        edges.push(("replay", next));
    }
    for index in 0..2 {
        if state.committed_requests[index] {
            let mut duplicate = state;
            if rules == Rules::DuplicateInflatesRights {
                duplicate.available += 1;
            }
            edges.push(("duplicate_same_payload", duplicate));
            edges.push(("duplicate_changed_payload_rejected", state));
        } else if state.slot.is_none() && state.available == 1 {
            let mut next = state;
            next.pending = Some(Pending::Promote(index));
            edges.push(("start_promotion", next));
        } else {
            edges.push(("competing_slot_rejected", state));
        }
        match state.effects[index] {
            Effect::Authorized => {
                let mut claim = state;
                claim.pending = Some(Pending::Claim(index));
                edges.push(("start_claim", claim));
                let mut cancel = state;
                cancel.pending = Some(Pending::Cancel(index));
                edges.push(("start_cancel_before_send", cancel));
            }
            Effect::Dispatching => {
                if state.tickets[index] {
                    let mut unknown = state;
                    unknown.pending = Some(Pending::Unknown(index));
                    edges.push(("start_ambiguous_outcome", unknown));
                    match state.replies[index] {
                        Reply::Confirmed => {
                            let mut next = state;
                            next.pending = Some(Pending::Confirm(index));
                            edges.push(("start_confirm", next));
                        }
                        Reply::Rejected => {
                            let mut next = state;
                            next.pending = Some(Pending::Reject(index));
                            edges.push(("start_definitive_rejection", next));
                        }
                        Reply::None => {}
                    }
                    if state.sends[index] < 2 {
                        let mut success = state;
                        success.sends[index] += 1;
                        // An identical operation ID never changes a terminal provider result.
                        if success.provider[index] != Provider::Rejected {
                            success.provider[index] = Provider::Performed;
                            success.replies[index] = Reply::Confirmed;
                            edges.push(("send_success_with_reply", success));
                            success.replies[index] = Reply::None;
                            edges.push(("send_success_lost_ack", success));
                        }
                        if state.provider[index] != Provider::Performed {
                            let mut reject = state;
                            reject.sends[index] += 1;
                            reject.provider[index] = Provider::Rejected;
                            reject.replies[index] = Reply::Rejected;
                            edges.push(("send_definitive_rejection", reject));
                        }
                        if state.provider[index] == Provider::NotObserved {
                            let mut no_delivery = state;
                            no_delivery.sends[index] += 1;
                            no_delivery.replies[index] = Reply::None;
                            edges.push(("send_no_delivery_or_reply", no_delivery));
                        }
                    }
                }
            }
            Effect::Unknown => match state.provider[index] {
                Provider::Performed => {
                    let mut next = state;
                    next.pending = Some(Pending::Confirm(index));
                    edges.push(("reconcile_confirmed", next));
                }
                Provider::Rejected => {
                    let mut next = state;
                    next.pending = Some(Pending::Reject(index));
                    edges.push(("reconcile_definitive_rejection", next));
                }
                Provider::NotObserved => edges.push(("reconcile_inconclusive", state)),
            },
            Effect::Confirmed => edges.push(("duplicate_confirmed_reply", state)),
            Effect::Rejected => edges.push(("duplicate_rejected_reply", state)),
            Effect::Absent | Effect::Cancelled => {}
        }
    }
    edges
}

fn explore(rules: Rules) -> Result<Exploration, String> {
    let initial = State::default();
    let mut visited = BTreeSet::from([initial]);
    let mut queue = VecDeque::from([initial]);
    let mut result = Exploration::default();
    while let Some(state) = queue.pop_front() {
        invariant(&state)?;
        result.unknown_states += usize::from(state.effects.contains(&Effect::Unknown));
        result.crash_states += usize::from(state.mode == Mode::Down);
        result.replay_states += usize::from(state.replays > 0);
        for (action, next) in transitions(state, rules) {
            result.transitions += 1;
            result.duplicate_edges += usize::from(action.starts_with("duplicate_"));
            result.lost_ack_edges += usize::from(action == "send_success_lost_ack");
            invariant(&next).map_err(|reason| {
                format!("{action}: {reason}; predecessor={state:?}; successor={next:?}")
            })?;
            if visited.insert(next) {
                if visited.len() > 100_000 {
                    return Err("model exploration exceeded its explicit 100000-state cap".into());
                }
                queue.push_back(next);
            }
        }
    }
    result.states = visited.len();
    Ok(result)
}

#[test]
fn bounded_model_explores_crashes_duplicates_ambiguity_and_replay() -> Result<(), String> {
    let result = explore(Rules::Correct)?;
    assert!(result.states > 100);
    assert!(result.transitions > result.states);
    assert!(result.duplicate_edges > 0);
    assert!(result.lost_ack_edges > 0);
    assert!(result.unknown_states > 0);
    assert!(result.crash_states > 0);
    assert!(result.replay_states > 0);
    println!(
        "{{\"model\":\"two_branches_one_slot\",\"states\":{},\"transitions\":{},\"duplicate_edges\":{},\"lost_ack_edges\":{},\"unknown_states\":{},\"crash_states\":{},\"replay_states\":{}}}",
        result.states,
        result.transitions,
        result.duplicate_edges,
        result.lost_ack_edges,
        result.unknown_states,
        result.crash_states,
        result.replay_states
    );
    Ok(())
}

#[test]
fn model_detects_dispatch_before_durable_authorization() {
    let violation = explore(Rules::SendBeforeCommit).expect_err("unsafe model must be rejected");
    assert!(violation.contains("send preceded durable authorization"));
}

#[test]
fn model_detects_unknown_releasing_its_slot_and_right() {
    let violation = explore(Rules::ReleaseUnknown).expect_err("unsafe model must be rejected");
    assert!(violation.contains("unresolved operation lost its slot or reserved right"));
}

#[test]
fn model_detects_provider_calls_during_replay() {
    let violation =
        explore(Rules::DispatchDuringReplay).expect_err("unsafe model must be rejected");
    assert!(violation.contains("replay called an external provider"));
}

#[test]
fn model_detects_duplicate_request_inflating_rights() {
    let violation =
        explore(Rules::DuplicateInflatesRights).expect_err("unsafe model must be rejected");
    assert!(violation.contains("the initial single right is not conserved"));
}
