//! Explicit compositor boundary between modelled responses and confirmed receipts.
//!
//! This helper does not restrict the trusted host's raw Authority API. The host
//! must serialize mutations of its workflow across scheduler and authority calls.
use super::{RunId, RunReport, ScheduleError, Scheduler};
use chronos_codec::ObjectId;
use chronos_ledger::{Authority, EffectState, LedgerError, PromotionOutcome, PromotionRequest};
use chronos_state::{DomainId, execution::StepExit};
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum ResponseBindingError {
    Unconfirmed,
    Incomplete,
    BindingMismatch,
    Ledger(LedgerError),
    Scheduler(ScheduleError),
}
impl fmt::Display for ResponseBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unconfirmed => "effect lacks a confirmed durable receipt",
            Self::Incomplete => "receipt-dependent computation is incomplete",
            Self::BindingMismatch => "promotion does not match the receipt-bound run",
            Self::Ledger(_) => "receipt authority failed",
            Self::Scheduler(_) => "receipt rerun failed",
        })
    }
}
impl Error for ResponseBindingError {}
impl From<LedgerError> for ResponseBindingError {
    fn from(value: LedgerError) -> Self {
        Self::Ledger(value)
    }
}
impl From<ScheduleError> for ResponseBindingError {
    fn from(value: ScheduleError) -> Self {
        Self::Scheduler(value)
    }
}
/// One-use result of completing the affected subtree with a durable confirmed
/// receipt. It cannot be constructed from a caller-supplied report or cloned.
pub struct ConfirmedResponse {
    receipt: ObjectId,
    report: RunReport,
    scheduler: Scheduler,
    run: RunId,
    affected: Vec<ObjectId>,
    intent: ObjectId,
    domain: DomainId,
    epoch: u64,
}
impl ConfirmedResponse {
    pub fn receipt(&self) -> ObjectId {
        self.receipt
    }
    pub fn report(&self) -> &RunReport {
        &self.report
    }
    /// Promote through this explicit workflow guard. The application still owns
    /// interpretation of results and must serialize concurrent workflow changes.
    /// Ledger dependency, rights, generation and policy checks remain mandatory.
    pub async fn promote(
        self,
        authority: &mut Authority,
        request: PromotionRequest,
    ) -> Result<PromotionOutcome, ResponseBindingError> {
        if authority.domain() != self.domain
            || authority.epoch() != self.epoch
            || confirmed_receipt(authority, self.intent)? != self.receipt
        {
            return Err(ResponseBindingError::BindingMismatch);
        }
        let current = self.scheduler.drive(self.run).await?;
        if current.trace != self.report.trace {
            return Err(ResponseBindingError::BindingMismatch);
        }
        let worldline = request.branch.worldline();
        let matched = self.report.trace.graph.nodes.iter().any(|node| {
            self.affected.contains(&node.id)
                && node.branch == request.branch.id()
                && worldline.observation_root == Some(self.receipt)
                && worldline.code == Some(node.request.code)
                && worldline.profile == Some(node.request.profile)
                && worldline.security_scope == Some(node.request.security_scope)
                && worldline.policy == Some(node.request.policy)
        });
        if request.branch.domain() != self.domain || !matched {
            return Err(ResponseBindingError::BindingMismatch);
        }
        Ok(authority.promote(request)?)
    }
}
fn confirmed_receipt(
    authority: &Authority,
    intent: ObjectId,
) -> Result<ObjectId, ResponseBindingError> {
    if authority.effect_state(intent)? != EffectState::Confirmed {
        return Err(ResponseBindingError::Unconfirmed);
    }
    authority
        .latest_receipt(intent)?
        .ok_or(ResponseBindingError::Unconfirmed)
}
/// Replace the modelled input with the authority's exact latest confirmed receipt
/// and rerun every affected descendant. Receipt objects must already be reachable
/// in the scheduler repository (for example, by sharing the authority's CAS).
/// This function never sends or reconciles provider requests.
pub async fn rerun_confirmed_receipt(
    authority: &Authority,
    scheduler: &Scheduler,
    run: RunId,
    node: ObjectId,
    intent: ObjectId,
) -> Result<ConfirmedResponse, ResponseBindingError> {
    let receipt = confirmed_receipt(authority, intent)?;
    let domain = authority.domain();
    let epoch = authority.epoch();
    let affected = scheduler.replace_observation(run, node, receipt)?;
    let report = scheduler.drive(run).await?;
    if affected.iter().any(|id| !matches!(report.outcomes.get(id).map(|outcome|&outcome.exit),Some(StepExit::Completed{observation_root,..}) if *observation_root==receipt)) {
        return Err(ResponseBindingError::Incomplete);
    }
    if authority.domain() != domain
        || authority.epoch() != epoch
        || confirmed_receipt(authority, intent)? != receipt
    {
        return Err(ResponseBindingError::BindingMismatch);
    }
    Ok(ConfirmedResponse {
        receipt,
        report,
        scheduler: scheduler.clone(),
        run,
        affected,
        intent,
        domain,
        epoch,
    })
}
