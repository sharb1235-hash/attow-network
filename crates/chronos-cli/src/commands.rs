use crate::{
    CliError, Workspace,
    args::{Args, Command},
    bundle::Bundle,
    demo::{self, DemoOptions},
    display::Output,
    workspace::validate_id,
};
use chronos_codec::{Limits, ObjectId};
use chronos_ledger::{Authority, PromotionOutcome, PromotionRequest};
use chronos_local::{core::persistence::StoredStateView, owned_loopback::OwnedLoopbackFence};
use chronos_state::{FrozenBranch, StateRoot};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc};
const HELP: &str = "Attow Network 0.1.0\n\nattow init PATH [--json]\nattow run checkout|data-change [--workers 1..8] [--resume RUN] [--browser] [--pause-after-unknown] [--json]\nattow branch list [--json]\nattow compare BRANCH_A BRANCH_B [--json]\nattow promote BRANCH --decision ID --generation N --request-id ID [--json]\nattow replay RUN --offline [--json]\nattow verify RUN [--json]\nattow doctor [--json]\nattow peer serve --bind 127.0.0.1:0 [--duration-ms N] [--json]\nattow export RUN --output PATH [--json]\nattow import PATH --data-only [--json]\n\nUse --root PATH to select an initialized workspace. Exit codes: 0 success, 2 conflict, 3 unknown, 4 input, 5 infrastructure.\nReplay has no workers or dispatch. Browser is an optional owned fixture. Peer serve denies proposals and peers without an allowlist.\n";
pub async fn execute(args: Args, cwd: &Path) -> Result<Output, CliError> {
    match &args.command {
        Command::Help => return Ok(Output::success("help", json!({"help":HELP}))),
        Command::Version => {
            return Ok(Output::success(
                "version",
                json!({"version":"0.1.0","name":"Attow Network"}),
            ));
        }
        Command::Init(path) => {
            let workspace = Workspace::init(&cwd.join(path))?;
            return Ok(Output::success(
                "init",
                json!({"workspace":".attow","schema":1,"writer_id":workspace.writer_id()?.to_string(),"fence":"owned_same_host","data_only_imports":true}),
            ));
        }
        Command::Doctor => {
            let configured = ["ATTOW_NODE", "ATTOW_BROWSER_SIDECAR"].iter().all(|name| {
                std::env::var_os(name).is_some_and(|path| {
                    let path = std::path::PathBuf::from(path);
                    path.is_absolute() && path.is_file()
                })
            });
            let chromium_revision_present = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH")
                .is_some_and(|root| {
                    let root = std::path::PathBuf::from(root);
                    root.is_absolute()
                        && root.join("chromium-1248").is_dir()
                        && root.join("chromium_headless_shell-1248").is_dir()
                });
            let workspace = Workspace::discover(
                &args
                    .root
                    .as_ref()
                    .map_or_else(|| cwd.to_path_buf(), |root| cwd.join(root)),
            )
            .is_ok();
            return Ok(Output::success(
                "doctor",
                json!({"workspace_initialized":workspace,"browser":{"compiled":cfg!(feature="browser"),"configured":configured && chromium_revision_present,"bridge_paths_present":configured,"chromium_revision_present":chromium_revision_present,"versions_verified":false,"readiness":"not_probed","required_node":"24.21.0","required_playwright":"1.64.0","required_chromium_revision":"1248"},"offline_core":true,"distributed_fence_service":false}),
            ));
        }
        _ => {}
    }
    let workspace = match &args.root {
        Some(path) => Workspace::open(&cwd.join(path))?,
        None => Workspace::discover(cwd)?,
    };
    let offline_replay = matches!(&args.command, Command::Replay(_));
    match args.command {
        Command::Run {
            scenario,
            workers,
            resume,
            browser,
            pause_after_unknown,
        } => {
            let record = demo::run(
                &workspace,
                DemoOptions {
                    scenario,
                    workers,
                    resume,
                    browser,
                    pause_after_unknown,
                    boundary_hook: None,
                },
            )
            .await?;
            let bundle = Bundle::gather(&workspace, record.clone())?;
            bundle.write_default(&workspace)?;
            let mut output = Output::success("run", serde_json::to_value(&record)?);
            if record.phase == "unknown" {
                output.envelope.status = "unknown".into();
                output.exit_code = 3;
            }
            if record.phase == "rejected" {
                output.envelope.status = "conflict".into();
                output.exit_code = 2;
            }
            Ok(output)
        }
        Command::BranchList => {
            let mut branches = Vec::new();
            let mut total = 0usize;
            for run in workspace.list_runs()? {
                total += run.branches.len();
                let native = workspace.is_native(&run.run_id)?;
                for branch in &run.branches {
                    if branches.len() >= 256 {
                        continue;
                    }
                    let details = inspect_dependencies(&workspace, &run, branch);
                    branches.push(json!({"run_id":run.run_id,"branch_id":branch.branch_id,"descriptor":branch.descriptor,"label":branch.label,"state_root":branch.state_root,"observation_root":branch.observation_root,"phase":run.phase,"fidelity":run.fidelity,"native":native,"trace_roots":run.trace_roots,"recorded_execution":run.summary,"dependencies":details,"proposed_effect":{"payload":run.payload,"operation":run.operation,"intent":run.intent},"evidence":{"receipt":run.receipt,"response_descriptor":run.response_descriptor}}));
                }
            }
            Ok(Output::success(
                "branch",
                json!({"branches":branches,"total_branches":total,"truncated":total>256}),
            ))
        }
        Command::Compare(left, right) => {
            let (left_run, left_record) = find_branch(&workspace, &left)?;
            let (right_run, right_record) = find_branch(&workspace, &right)?;
            let left_bundle = Bundle::gather(&workspace, left_run)?;
            let right_bundle = Bundle::gather(&workspace, right_run)?;
            let left_root = StateRoot::decode(
                &left_bundle.objects[&validate_id(&left_record.state_root)?].bytes,
                &Limits::default(),
            )?;
            let right_root = StateRoot::decode(
                &right_bundle.objects[&validate_id(&right_record.state_root)?].bytes,
                &Limits::default(),
            )?;
            let differences = chronos_state::diff(&left_root, &right_root)?;
            let rows:Vec<_>=differences.iter().take(128).map(|row|json!({"key_hex":hex(&row.key),"before":row.before.as_ref().and_then(|cell|cell.value.as_deref()).map(preview),"after":row.after.as_ref().and_then(|cell|cell.value.as_deref()).map(preview)})).collect();
            Ok(Output::success(
                "compare",
                json!({"left":left,"right":right,"differences":rows,"total_differences":differences.len(),"truncated":differences.len()>128}),
            ))
        }
        Command::Replay(id) | Command::Verify(id) => {
            let offline = offline_replay;
            let record = workspace.load_run(&id)?;
            let bundle = Bundle::gather(&workspace, record)?;
            let mut result = bundle.verify()?;
            result["offline"] = json!(true);
            if !offline && workspace.is_native(&id)? {
                let authority = Authority::inspect(&workspace.authority_path())?;
                let report = authority.rebuild_and_verify()?;
                result["native_promotions"] = verify_native(&authority, &bundle)?;
                result["local_authority_history"] = json!({"events":report.events,"head":report.head.to_string(),"current_state_root":report.state_root.to_string(),"materialized_tables":report.materialized_tables});
            } else {
                result["local_authority_history"] = json!("not_checked");
            }
            Ok(Output::success(
                if offline { "replay" } else { "verify" },
                result,
            ))
        }
        Command::Export { run, output } => {
            let record = workspace.load_run(&run)?;
            let bundle = Bundle::gather(&workspace, record)?;
            let bytes = bundle.encode()?.len();
            bundle.write(&cwd.join(output))?;
            Ok(Output::success(
                "export",
                json!({"run_id":run,"format":"attow-run-bundle","schema":1,"data_only":true,"bytes":bytes}),
            ))
        }
        Command::Import(path) => {
            let bundle = Bundle::read(&cwd.join(path))?;
            bundle.import(&workspace)?;
            bundle.write_default(&workspace)?;
            Ok(Output::success(
                "import",
                json!({"run_id":bundle.record.run_id,"data_only":true,"activation":false,"bundle_path":bundle.record.bundle_path}),
            ))
        }
        Command::Promote {
            branch,
            decision,
            generation,
            request_id,
        } => {
            let (record, branch_record) = find_branch(&workspace, &branch)?;
            if !workspace.is_native(&record.run_id)? {
                return Err(CliError::input("imported data cannot be promoted"));
            }
            // The command proposes an existing native branch; it never dispatches
            // and cannot select a receipt-dependent descriptor outside its guard.
            let payload = validate_id(
                record
                    .payload
                    .as_deref()
                    .ok_or_else(|| CliError::input("branch lacks a prepared payload"))?,
            )?;
            let writer = workspace.writer_id()?;
            let fence = OwnedLoopbackFence::open(&workspace.fence_path(), writer)?;
            let mut authority =
                Authority::open_federated(&workspace.authority_path(), writer, &fence)?;
            let store = chronos_store::Store::open_read_only(&workspace.authority_path())?;
            let frozen = FrozenBranch::reopen(
                &StoredStateView(&store),
                validate_id(&branch_record.descriptor)?,
                authority.domain(),
            )?;
            let outcome = authority.promote(PromotionRequest {
                id: validate_id(&request_id)?,
                payload,
                branch: frozen,
                decision: validate_id(&decision)?,
                generation,
            })?;
            let mut output = Output::success(
                "promote",
                json!({"branch_id":branch,"request_id":request_id,"dispatch_calls":0}),
            );
            match outcome {
                PromotionOutcome::LocalCommitted { seq } => {
                    output.envelope.data["state"] = json!("local_committed");
                    output.envelope.data["sequence"] = json!(seq);
                }
                PromotionOutcome::AcceptedPending { seq, intent } => {
                    output.envelope.data["state"] = json!("pending");
                    output.envelope.data["sequence"] = json!(seq);
                    output.envelope.data["intent"] = json!(intent.to_string());
                }
                PromotionOutcome::Conflict(report) => {
                    output.exit_code = 2;
                    output.envelope.status = "conflict".into();
                    output.envelope.data["state"] = json!("conflict");
                    output.envelope.data["reasons"] = json!(report.reasons);
                }
            }
            Ok(output)
        }
        Command::PeerServe { bind, duration_ms } => {
            let address = bind
                .parse::<std::net::SocketAddr>()
                .map_err(|_| CliError::input("invalid bind address"))?;
            if !address.ip().is_loopback() {
                return Err(CliError::input(
                    "CLI fixture peer must bind a loopback address",
                ));
            }
            let mut seed = [0; 32];
            getrandom::fill(&mut seed).map_err(CliError::infrastructure)?;
            let authority = Authority::inspect(&workspace.authority_path())?;
            let node = chronos_transport::PeerNode::bind(chronos_transport::PeerConfig {
                secret_key: chronos_transport::SecretKey::from_bytes(&seed),
                bind_addr: address,
                tenant: workspace.writer_id()?,
                domain: ObjectId(authority.domain().0),
                permissions: BTreeMap::new(),
                limits: Default::default(),
                objects: Arc::new(chronos_transport::SqliteObjectRepository::open(
                    &workspace.authority_path(),
                )?),
                authority: Arc::new(chronos_transport::DenyProposals),
            })
            .await?;
            let address = node.address();
            tokio::time::sleep(std::time::Duration::from_millis(duration_ms)).await;
            node.close().await;
            Ok(Output::success(
                "peer",
                json!({"peer_id":hex(&address.peer),"address":address.address.to_string(),"duration_ms":duration_ms,"allowed_peers":0,"proposals":false,"closed":true}),
            ))
        }
        _ => Err(CliError::input("invalid command")),
    }
}
fn find_branch(
    workspace: &Workspace,
    id: &str,
) -> Result<(crate::RunRecord, crate::BranchRecord), CliError> {
    validate_id(id)?;
    let mut found = None;
    for run in workspace.list_runs()? {
        if let Some(branch) = run.branches.iter().find(|branch| branch.branch_id == id) {
            if found.is_some() {
                return Err(CliError::input("ambiguous branch identifier"));
            }
            found = Some((run.clone(), branch.clone()));
        }
    }
    found.ok_or_else(|| CliError::input("branch is absent"))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn preview(bytes: &[u8]) -> serde_json::Value {
    json!({"bytes":bytes.len(),"hex_prefix":hex(&bytes[..bytes.len().min(4096)]),"truncated":bytes.len()>4096})
}

fn inspect_dependencies(
    workspace: &Workspace,
    run: &crate::RunRecord,
    branch: &crate::BranchRecord,
) -> serde_json::Value {
    let inspect = || -> Result<serde_json::Value, CliError> {
        let store = chronos_store::Store::open_read_only(&workspace.authority_path())?;
        let initial = StateRoot::decode(
            &store.get(validate_id(
                run.initial_root
                    .as_deref()
                    .ok_or_else(|| CliError::input("missing initial root"))?,
            )?)?,
            &Limits::default(),
        )?;
        let frozen = FrozenBranch::reopen(
            &StoredStateView(&store),
            validate_id(&branch.descriptor)?,
            initial.domain(),
        )?;
        let items: Vec<_> = frozen
            .dependencies()
            .iter()
            .take(64)
            .map(|dependency| match dependency {
                chronos_state::Dependency::Key { key, version } => {
                    json!({"kind":"key","key_hex":hex(key),"version":format!("{version:?}")})
                }
                chronos_state::Dependency::Namespace { epoch } => {
                    json!({"kind":"namespace","epoch":epoch})
                }
            })
            .collect();
        Ok(
            json!({"available":true,"items":items,"count":frozen.dependencies().len(),"truncated":frozen.dependencies().len()>64}),
        )
    };
    inspect().unwrap_or_else(|_|json!({"available":false,"reason":"descriptor not yet admitted or failed verification"}))
}

fn verify_native(authority: &Authority, bundle: &Bundle) -> Result<serde_json::Value, CliError> {
    let record = &bundle.record;
    let mismatch =
        || CliError::input("native run record does not match authoritative promotion history");
    let required = |value: &Option<String>| -> Result<ObjectId, CliError> {
        validate_id(value.as_deref().ok_or_else(mismatch)?)
    };
    let selected = record
        .branches
        .iter()
        .find(|branch| Some(&branch.branch_id) == record.selected_branch.as_ref())
        .ok_or_else(mismatch)?;
    let original = authority
        .promotion_record(required(&record.request_id)?)?
        .ok_or_else(mismatch)?;
    if original.payload != required(&record.payload)?
        || original.branch != validate_id(&selected.descriptor)?
        || original.decision != required(&record.decision)?
        || original.generation != record.generation
        || original.state_root != validate_id(&selected.state_root)?
    {
        return Err(mismatch());
    }
    match (&original.outcome, record.scenario.as_str()) {
        (PromotionOutcome::LocalCommitted { .. }, "data-change") => {}
        (PromotionOutcome::AcceptedPending { intent, .. }, "checkout")
            if Some(intent.to_string()) == record.intent => {}
        _ => return Err(mismatch()),
    }
    let mut verified = vec![
        json!({"request_id":original.request_id.to_string(),"sequence":original.sequence,"state_root":original.state_root.to_string(),"effect":"pending_or_local_commit"}),
    ];
    if let Some(id) = &record.response_request_id {
        let response = authority.promotion_record(validate_id(id)?)?;
        if let Some(response) = response {
            let descriptor = required(&record.response_descriptor)?;
            let initial = StateRoot::decode(
                &bundle.objects[&required(&record.initial_root)?].bytes,
                &Limits::default(),
            )?;
            let branch = FrozenBranch::reopen(
                &crate::bundle::BundleReader(&bundle.objects),
                descriptor,
                initial.domain(),
            )?;
            if response.payload != required(&record.response_payload)?
                || response.branch != descriptor
                || response.decision != required(&record.response_decision)?
                || Some(response.generation) != record.response_generation
                || response.state_root != branch.base().apply(branch.overlay())?.object_id()?
                || !matches!(response.outcome, PromotionOutcome::LocalCommitted { .. })
            {
                return Err(mismatch());
            }
            verified.push(json!({"request_id":response.request_id.to_string(),"sequence":response.sequence,"state_root":response.state_root.to_string(),"effect":"local_commit"}));
        } else if record.phase == "completed" {
            return Err(mismatch());
        }
    }
    if let Some(intent) = &record.intent
        && authority
            .latest_receipt(validate_id(intent)?)?
            .map(|id| id.to_string())
            != record.receipt
    {
        return Err(mismatch());
    }
    Ok(json!(verified))
}
