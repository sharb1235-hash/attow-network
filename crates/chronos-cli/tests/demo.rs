use attow_cli::{
    demo::{self, DemoBoundary, DemoOptions, Scenario},
    workspace::Workspace,
};
// Serialize full fixture scenarios; each graph still exercises concurrent workers.
static DEMO_SERIAL: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
fn options(scenario: Scenario) -> DemoOptions {
    DemoOptions {
        scenario,
        workers: 2,
        resume: None,
        browser: false,
        pause_after_prepared: false,
        pause_after_unknown: false,
        boundary_hook: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmed_effect_does_not_silently_rebase_across_another_run() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(dir.path())?;
    let mut start = options(Scenario::Checkout);
    start.pause_after_unknown = true;
    let first = demo::run(&workspace, start).await?;
    let other = demo::run(&workspace, options(Scenario::DataChange)).await?;
    let mut resume = options(Scenario::Checkout);
    resume.resume = Some(first.run_id.clone());
    assert!(matches!(
        demo::run(&workspace, resume).await,
        Err(attow_cli::CliError::Conflict(_))
    ));
    let saved = workspace.load_run(&first.run_id)?;
    assert_eq!(saved.phase, "confirmed");
    assert!(saved.response_descriptor.is_none());
    let authority = chronos_ledger::Authority::inspect(&workspace.authority_path())?;
    assert_eq!(
        authority.current_root()?.object_id()?.to_string(),
        other.managed_root.expect("completed managed root")
    );
    assert_eq!(
        authority.effect_state(saved.intent.as_ref().expect("authorized intent").parse()?)?,
        chronos_ledger::EffectState::Confirmed
    );
    attow_cli::bundle::Bundle::gather(&workspace, saved)?.verify()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_rejection_remains_rejected_without_a_response_commit() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    use chronos_adapters::provider_simulator::{SimulatorConfig, SimulatorServer};
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(dir.path())?;
    {
        let server =
            SimulatorServer::start(&workspace.provider_path(), &SimulatorConfig::default())?;
        server
            .controller()
            .hold(chronos_codec::ObjectId([77; 32]), 10)?;
    }
    let mut start = options(Scenario::Checkout);
    start.pause_after_unknown = true;
    let pending = demo::run(&workspace, start).await?;
    assert_eq!(pending.phase, "unknown");
    let mut resume = options(Scenario::Checkout);
    resume.resume = Some(pending.run_id.clone());
    let rejected = demo::run(&workspace, resume).await?;
    assert_eq!(rejected.phase, "rejected");
    assert!(rejected.response_descriptor.is_none());
    assert_eq!(rejected.summary["provider"]["accepted"], 0);
    assert_eq!(rejected.summary["provider"]["dispatch_requests"], 1);
    attow_cli::bundle::Bundle::gather(&workspace, rejected)?.verify()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepared_resume_rejects_new_base_but_committed_resume_uses_historical_root() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    for point in ["prepared", "response-committed"] {
        let dir = tempfile::tempdir()?;
        let workspace = Workspace::init(dir.path())?;
        let child = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "abrupt_demo_child"])
            .env("ATTOW_DEMO_CRASH_WORKSPACE", workspace.root())
            .env("ATTOW_DEMO_CRASH_POINT", point)
            .output()?;
        assert!(!child.status.success());
        assert_eq!(
            std::fs::read_to_string(workspace.data_dir().join("demo-crash-marker"))?,
            point
        );
        let first = workspace.list_runs()?.remove(0);
        let other = demo::run(&workspace, options(Scenario::DataChange)).await?;
        let mut resume = options(Scenario::Checkout);
        resume.resume = Some(first.run_id.clone());
        let result = demo::run(&workspace, resume).await;
        if point == "prepared" {
            assert!(matches!(result, Err(attow_cli::CliError::Conflict(_))));
            let authority = chronos_ledger::Authority::inspect(&workspace.authority_path())?;
            assert!(
                authority
                    .promotion_record(first.request_id.expect("prepared request").parse()?)?
                    .is_none()
            );
        } else {
            let completed = result?;
            assert_eq!(completed.phase, "completed");
            assert_ne!(completed.managed_root, other.managed_root);
            attow_cli::bundle::Bundle::gather(&workspace, completed)?.verify()?;
        }
        let authority = chronos_ledger::Authority::inspect(&workspace.authority_path())?;
        assert_eq!(
            authority.current_root()?.object_id()?.to_string(),
            other.managed_root.expect("completed managed root")
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn five_data_change_branches_execute_real_workers_and_promote_over_quic() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(&dir.path().join("owned workspace with spaces"))?;
    let record = demo::run(&workspace, options(Scenario::DataChange)).await?;
    assert_eq!(record.branches.len(), 5);
    assert_eq!(record.phase, "completed");
    assert!(record.intent.is_none());
    assert_eq!(record.summary["logical_nodes"], 10);
    assert_eq!(record.summary["executed"], 2);
    assert_eq!(record.summary["reused"], 8);
    assert!(
        record
            .events
            .iter()
            .any(|event| event["kind"] == "peer_transfer" && event["durable"] == true)
    );
    assert_ne!(record.initial_root, record.managed_root);
    assert!(!record.trace_roots.is_empty());
    let saved = Workspace::open(workspace.root())?.load_run(&record.run_id)?;
    assert_eq!(saved.managed_root, record.managed_root);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_response_stays_unknown_then_resumes_the_same_logical_purchase() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(dir.path())?;
    let mut start = options(Scenario::Checkout);
    start.pause_after_unknown = true;
    let unknown = demo::run(&workspace, start).await?;
    assert_eq!(unknown.phase, "unknown");
    assert!(unknown.intent.is_some());
    assert!(unknown.receipt.is_some());
    assert_eq!(unknown.summary["provider"]["accepted"], 1);
    {
        use chronos_adapters::provider_simulator::{Fault, SimulatorConfig, SimulatorServer};
        let server =
            SimulatorServer::start(&workspace.provider_path(), &SimulatorConfig::default())?;
        server.controller().set_fault(Fault::HideNextLookup)?;
    }
    let mut retry_lookup = options(Scenario::Checkout);
    retry_lookup.resume = Some(unknown.run_id.clone());
    let no_progress = demo::run(&workspace, retry_lookup).await?;
    assert_eq!(
        no_progress.phase, "unknown",
        "a delayed NotFound cannot authorize another send"
    );
    assert_eq!(no_progress.intent, unknown.intent);
    assert_eq!(no_progress.summary["provider"]["dispatch_requests"], 1);
    let mut resume = options(Scenario::Checkout);
    resume.resume = Some(unknown.run_id.clone());
    let confirmed = demo::run(&workspace, resume).await?;
    assert_eq!(confirmed.phase, "completed");
    assert_eq!(confirmed.intent, unknown.intent);
    assert_eq!(confirmed.operation, unknown.operation);
    assert_eq!(confirmed.summary["provider"]["accepted"], 1);
    assert_eq!(confirmed.summary["provider"]["dispatch_requests"], 1);
    assert!(confirmed.response_descriptor.is_some());
    assert!(
        confirmed
            .events
            .iter()
            .any(|event| event["kind"] == "receipt_rerun")
    );
    let bundle = attow_cli::bundle::Bundle::gather(&workspace, confirmed)?;
    bundle.verify()?;
    Ok(())
}

#[test]
fn abrupt_demo_child() -> Result {
    let Some(path) = std::env::var_os("ATTOW_DEMO_CRASH_WORKSPACE") else {
        return Ok(());
    };
    let workspace = Workspace::open(std::path::Path::new(&path))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut start = options(Scenario::Checkout);
    start.boundary_hook = Some(|point| {
        let name = match point {
            DemoBoundary::AfterPrepared => "prepared",
            DemoBoundary::AfterIntentSaved => "intent",
            DemoBoundary::AfterGatewayReturned => "gateway",
            DemoBoundary::AfterResponsePrepared => "response-prepared",
            DemoBoundary::AfterResponseCommitted => "response-committed",
        };
        if std::env::var("ATTOW_DEMO_CRASH_POINT").as_deref() == Ok(name) {
            use std::io::Write;
            let root = std::path::PathBuf::from(
                std::env::var_os("ATTOW_DEMO_CRASH_WORKSPACE").expect("child workspace"),
            );
            let mut marker =
                std::fs::File::create(root.join(".attow/demo-crash-marker")).expect("crash marker");
            marker
                .write_all(name.as_bytes())
                .expect("write crash marker");
            marker.sync_all().expect("sync crash marker");
            std::process::abort();
        }
    });
    runtime.block_on(demo::run(&workspace, start))?;
    panic!("requested crash boundary was not reached")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abrupt_process_restart_preserves_request_intent_and_provider_operation() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    for point in [
        "prepared",
        "intent",
        "gateway",
        "response-prepared",
        "response-committed",
    ] {
        let dir = tempfile::tempdir()?;
        let workspace = Workspace::init(&dir.path().join("restart path with spaces"))?;
        let child = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "abrupt_demo_child", "--nocapture"])
            .env("ATTOW_DEMO_CRASH_WORKSPACE", workspace.root())
            .env("ATTOW_DEMO_CRASH_POINT", point)
            .output()?;
        assert!(
            !child.status.success(),
            "child must terminate abruptly at {point}"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.data_dir().join("demo-crash-marker"))?,
            point
        );
        assert!(
            !String::from_utf8_lossy(&child.stderr)
                .contains("requested crash boundary was not reached")
        );
        let records = workspace.list_runs()?;
        assert_eq!(records.len(), 1);
        let before = &records[0];
        assert_eq!(
            before.branches.len(),
            5,
            "child reached the persisted preparation boundary"
        );
        assert_eq!(
            before.phase,
            match point {
                "prepared" => "prepared",
                "intent" | "gateway" => "authorized",
                _ => "response_prepared",
            }
        );
        let mut resume = options(Scenario::Checkout);
        resume.resume = Some(before.run_id.clone());
        let after = demo::run(&workspace, resume).await?;
        assert_eq!(after.phase, "completed");
        assert_eq!(before.request_id, after.request_id);
        assert_eq!(before.operation, after.operation);
        if before.intent.is_some() {
            assert_eq!(before.intent, after.intent);
        }
        assert_eq!(after.summary["provider"]["accepted"], 1);
        assert_eq!(after.summary["provider"]["dispatch_requests"], 1);
        attow_cli::bundle::Bundle::gather(&workspace, after)?.verify()?;
    }
    Ok(())
}

#[cfg(feature = "browser")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_checkout_uses_five_actual_owned_observations() -> Result {
    let _scenario = DEMO_SERIAL.acquire().await?;
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(dir.path())?;
    let mut start = options(Scenario::Checkout);
    start.browser = true;
    start.pause_after_unknown = true;
    let record = demo::run(&workspace, start).await?;
    assert_eq!(record.phase, "unknown");
    assert_eq!(record.summary["logical_nodes"], 15);
    assert_eq!(record.summary["executed"], 7);
    assert_eq!(record.summary["reused"], 8);
    assert_eq!(record.fidelity["browser"], "OwnedEnvironment");
    let observed: std::collections::BTreeSet<_> = record
        .branches
        .iter()
        .map(|b| b.observation_root.as_ref().expect("branch observation"))
        .collect();
    assert_eq!(
        observed.len(),
        5,
        "each alternative restored its own browser storage snapshot"
    );
    let store = chronos_adapters::AdapterStore::open(&workspace.authority_path())?;
    for (index, branch) in record.branches.iter().enumerate() {
        let object = store.get_object(
            branch
                .observation_root
                .as_ref()
                .expect("branch observation")
                .parse()?,
        )?;
        let chronos_codec::Value::Array(fields) = object.payload else {
            panic!("observation envelope")
        };
        assert_eq!(
            fields[2],
            chronos_codec::Value::Text("OwnedEnvironment".into())
        );
        assert_eq!(fields[6], chronos_codec::Value::Text("snapshot".into()));
        let chronos_codec::Value::Bytes(bytes) = &fields[7] else {
            panic!("snapshot JSON")
        };
        let snapshot: serde_json::Value = serde_json::from_slice(bytes)?;
        assert!(
            snapshot["local"]
                .as_array()
                .expect("owned snapshot local storage")
                .contains(&serde_json::json!([
                    "alternative",
                    format!("owned-alternative-{}", index + 1)
                ]))
        );
    }
    Ok(())
}
