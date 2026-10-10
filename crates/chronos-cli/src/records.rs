use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BranchRecord {
    pub branch_id: String,
    pub descriptor: String,
    pub label: String,
    pub state_root: String,
    pub observation_root: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub schema: u32,
    pub run_id: String,
    pub scenario: String,
    pub phase: String,
    pub initial_root: Option<String>,
    pub managed_root: Option<String>,
    pub selected_branch: Option<String>,
    pub branches: Vec<BranchRecord>,
    pub policy: Option<String>,
    pub decision: Option<String>,
    pub generation: u64,
    pub request_id: Option<String>,
    pub payload: Option<String>,
    pub operation: Option<String>,
    pub intent: Option<String>,
    pub receipt: Option<String>,
    pub response_descriptor: Option<String>,
    pub response_request_id: Option<String>,
    pub response_payload: Option<String>,
    pub response_decision: Option<String>,
    pub response_generation: Option<u64>,
    pub trace_roots: Vec<String>,
    pub fidelity: BTreeMap<String, String>,
    pub events: Vec<serde_json::Value>,
    pub summary: serde_json::Value,
    pub bundle_path: String,
}
impl RunRecord {
    pub fn new(run_id: String, scenario: String) -> Self {
        Self {
            schema: 1,
            bundle_path: format!(".attow/bundles/{run_id}.attow"),
            run_id,
            scenario,
            phase: "created".into(),
            initial_root: None,
            managed_root: None,
            selected_branch: None,
            branches: Vec::new(),
            policy: None,
            decision: None,
            generation: 0,
            request_id: None,
            payload: None,
            operation: None,
            intent: None,
            receipt: None,
            response_descriptor: None,
            response_request_id: None,
            response_payload: None,
            response_decision: None,
            response_generation: None,
            trace_roots: Vec::new(),
            fidelity: BTreeMap::new(),
            events: Vec::new(),
            summary: serde_json::json!({}),
        }
    }
    pub fn validate(&self) -> Result<(), crate::CliError> {
        use crate::{CliError, workspace::validate_id};
        validate_id(&self.run_id)?;
        if self.schema != 1
            || !matches!(self.scenario.as_str(), "checkout" | "data-change")
            || !matches!(
                self.phase.as_str(),
                "created"
                    | "prepared"
                    | "authorized"
                    | "unknown"
                    | "confirmed"
                    | "response_prepared"
                    | "completed"
                    | "rejected"
            )
            || self.bundle_path != format!(".attow/bundles/{}.attow", self.run_id)
            || self.branches.len() > 64
            || self.trace_roots.len() > 16
            || self.events.len() > 4096
            || self.fidelity.len() > 16
        {
            return Err(CliError::input("invalid bounded run record"));
        }
        if !self.summary.is_object()
            || self.events.iter().any(|event| {
                !event.is_object()
                    || event
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|kind| kind.is_empty() || kind.len() > 64)
            })
        {
            return Err(CliError::input("invalid run diagnostics"));
        }
        if self.phase != "created" {
            if self.branches.is_empty()
                || self.trace_roots.is_empty()
                || [
                    &self.initial_root,
                    &self.managed_root,
                    &self.selected_branch,
                    &self.policy,
                    &self.decision,
                    &self.request_id,
                    &self.payload,
                ]
                .iter()
                .any(|value| value.is_none())
            {
                return Err(CliError::input(
                    "run phase lacks prepared execution evidence",
                ));
            }
            if self.scenario == "checkout" && self.operation.is_none() {
                return Err(CliError::input("checkout lacks operation identity"));
            }
        }
        if self.scenario == "data-change"
            && (self.intent.is_some()
                || self.operation.is_some()
                || self.receipt.is_some()
                || self.response_descriptor.is_some()
                || matches!(
                    self.phase.as_str(),
                    "authorized" | "unknown" | "confirmed" | "response_prepared"
                ))
        {
            return Err(CliError::input(
                "data-change cannot claim an external effect",
            ));
        }
        if self.scenario == "checkout"
            && matches!(
                self.phase.as_str(),
                "authorized" | "unknown" | "confirmed" | "response_prepared" | "completed"
            )
            && self.intent.is_none()
        {
            return Err(CliError::input("effect phase lacks a durable intent"));
        }
        if self.scenario == "checkout"
            && matches!(
                self.phase.as_str(),
                "unknown" | "confirmed" | "response_prepared" | "completed"
            )
            && self.receipt.is_none()
        {
            return Err(CliError::input("effect phase lacks a durable receipt"));
        }
        let response_fields = [
            &self.response_descriptor,
            &self.response_request_id,
            &self.response_payload,
            &self.response_decision,
        ];
        let any_response = response_fields.iter().any(|value| value.is_some())
            || self.response_generation.is_some();
        if (any_response
            || (self.scenario == "checkout"
                && matches!(self.phase.as_str(), "response_prepared" | "completed")))
            && (response_fields.iter().any(|value| value.is_none())
                || self.response_generation.is_none()
                || self.trace_roots.len() < 2)
        {
            return Err(CliError::input(
                "response phase lacks its recorded transition",
            ));
        }
        for value in [
            &self.initial_root,
            &self.managed_root,
            &self.selected_branch,
            &self.policy,
            &self.decision,
            &self.request_id,
            &self.payload,
            &self.operation,
            &self.intent,
            &self.receipt,
            &self.response_descriptor,
            &self.response_request_id,
            &self.response_payload,
            &self.response_decision,
        ]
        .into_iter()
        .flatten()
        {
            validate_id(value)?;
        }
        let mut branches = std::collections::BTreeSet::new();
        for branch in &self.branches {
            for value in [&branch.branch_id, &branch.descriptor, &branch.state_root] {
                validate_id(value)?;
            }
            if let Some(id) = &branch.observation_root {
                validate_id(id)?;
            }
            if !branches.insert(&branch.branch_id) || branch.label.len() > 128 {
                return Err(CliError::input("invalid branch record"));
            }
        }
        if self
            .selected_branch
            .as_ref()
            .is_some_and(|id| !branches.contains(id))
        {
            return Err(CliError::input("selected branch is absent"));
        }
        for id in &self.trace_roots {
            validate_id(id)?;
        }
        for (key, value) in &self.fidelity {
            if key.len() > 64 || value.len() > 128 {
                return Err(CliError::input("invalid fidelity label"));
            }
        }
        if serde_json::to_vec(self)?.len() > crate::workspace::MAX_RECORD_BYTES {
            return Err(CliError::input("run record exceeds byte limit"));
        }
        Ok(())
    }
}
