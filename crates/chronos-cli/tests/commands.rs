use std::{error::Error, process::Command};
fn invoke(
    path: &std::path::Path,
    args: &[&str],
) -> Result<(i32, serde_json::Value), Box<dyn Error>> {
    let result = Command::new(env!("CARGO_BIN_EXE_attow"))
        .current_dir(path)
        .args(args)
        .output()?;
    let value = serde_json::from_slice(&result.stdout)?;
    Ok((result.status.code().ok_or("no exit")?, value))
}
#[test]
fn binary_init_doctor_list_and_bad_flags_have_stable_json() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let (code, output) = invoke(dir.path(), &["init", "workspace with spaces", "--json"])?;
    assert_eq!(code, 0);
    assert_eq!(output["schema"], 1);
    assert_eq!(output["command"], "init");
    assert_eq!(output["status"], "success");
    let root = dir.path().join("workspace with spaces");
    let (code, output) = invoke(&root, &["branch", "list", "--json"])?;
    assert_eq!(code, 0);
    assert_eq!(output["data"]["branches"], serde_json::json!([]));
    let result = Command::new(env!("CARGO_BIN_EXE_attow"))
        .current_dir(&root)
        .env_remove("ATTOW_NODE")
        .env_remove("ATTOW_BROWSER_SIDECAR")
        .env_remove("PLAYWRIGHT_BROWSERS_PATH")
        .args(["doctor", "--json"])
        .output()?;
    assert!(result.status.success());
    let doctor: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    assert_eq!(doctor["data"]["browser"]["configured"], false);
    let (code, output) = invoke(&root, &["run", "checkout", "--force", "--json"])?;
    assert_eq!(code, 4);
    assert_eq!(output["status"], "input");
    Ok(())
}

#[test]
fn doctor_reports_the_canonical_demo_execution_profile() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let (code, doctor) = invoke(directory.path(), &["doctor", "--json"])?;
    assert_eq!(code, 0);
    let profile = chronos_sandbox::ExecutionProfile::new(50_000, 65_536, 1024)?;
    assert_eq!(
        doctor["data"]["runtime_profile_hash"],
        profile.profile_hash.to_string()
    );
    assert_eq!(doctor["data"]["runtime_profile"]["fuel"], 50_000);
    assert_eq!(doctor["data"]["runtime_profile"]["memory_bytes"], 65_536);
    assert_eq!(doctor["data"]["runtime_profile"]["output_bytes"], 1024);
    Ok(())
}

#[test]
fn preparation_is_inspectable_before_any_promotion_or_effect_and_resume_keeps_ids()
-> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    invoke(dir.path(), &["init", ".", "--json"])?;
    let (code, prepared) = invoke(
        dir.path(),
        &["run", "checkout", "--pause-after-prepared", "--json"],
    )?;
    assert_eq!(code, 0, "preparation is a completed inspection checkpoint");
    let data = &prepared["data"];
    assert_eq!(data["phase"], "prepared");
    assert!(data["intent"].is_null());
    let id = data["run_id"].as_str().ok_or("run id")?;
    let ws = attow_cli::Workspace::open(dir.path())?;
    let authority = chronos_ledger::Authority::inspect(&ws.authority_path())?;
    assert!(
        authority
            .promotion_record(data["request_id"].as_str().ok_or("request")?.parse()?)?
            .is_none()
    );
    assert_eq!(
        authority.current_root()?.object_id()?.to_string(),
        data["initial_root"]
    );
    drop(authority);
    {
        use chronos_adapters::provider_simulator::{SimulatorConfig, SimulatorServer};
        let server = SimulatorServer::start(&ws.provider_path(), &SimulatorConfig::default())?;
        let stats = server.controller().stats()?;
        assert_eq!(stats.accepted, 0);
        assert_eq!(stats.dispatch_requests, 0);
    }
    let (code, listed) = invoke(dir.path(), &["branch", "list", "--json"])?;
    assert_eq!(code, 0);
    assert_eq!(listed["data"]["total_branches"], 5);
    let left = data["branches"][0]["branch_id"].as_str().ok_or("left")?;
    let right = data["branches"][1]["branch_id"].as_str().ok_or("right")?;
    let (code, compared) = invoke(dir.path(), &["compare", left, right, "--json"])?;
    assert_eq!(code, 0);
    assert_eq!(compared["data"]["total_differences"], 1);
    let (code, verified) = invoke(dir.path(), &["verify", id, "--json"])?;
    assert_eq!(code, 0);
    assert_eq!(verified["data"]["native_promotions"], serde_json::json!([]));
    let (code, unknown) = invoke(
        dir.path(),
        &[
            "run",
            "checkout",
            "--resume",
            id,
            "--pause-after-unknown",
            "--json",
        ],
    )?;
    assert_eq!(code, 3);
    assert_eq!(unknown["data"]["request_id"], data["request_id"]);
    assert_eq!(unknown["data"]["operation"], data["operation"]);
    let (code, confirmed) = invoke(dir.path(), &["run", "checkout", "--resume", id, "--json"])?;
    assert_eq!(code, 0);
    assert_eq!(confirmed["data"]["intent"], unknown["data"]["intent"]);
    let (code, _) = invoke(
        dir.path(),
        &[
            "run",
            "checkout",
            "--pause-after-prepared",
            "--pause-after-unknown",
            "--json",
        ],
    )?;
    assert_eq!(code, 4);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_prepared_absence_verification_rejects_substituted_logical_identities()
-> Result<(), Box<dyn Error>> {
    use attow_cli::{
        args::{Args, Command},
        commands,
        demo::{self, DemoOptions, Scenario},
    };
    let dir = tempfile::tempdir()?;
    let workspace = attow_cli::Workspace::init(dir.path())?;
    let prepared = demo::run(
        &workspace,
        DemoOptions {
            scenario: Scenario::Checkout,
            workers: 2,
            resume: None,
            browser: false,
            pause_after_prepared: true,
            pause_after_unknown: false,
            boundary_hook: None,
        },
    )
    .await?;
    let connection = rusqlite::Connection::open(workspace.data_dir().join("catalog.sqlite"))?;
    for field in ["request_id", "decision", "operation"] {
        let mut record = prepared.clone();
        match field {
            "request_id" => record.request_id = Some("fe".repeat(32)),
            "decision" => record.decision = Some("fe".repeat(32)),
            "operation" => record.operation = Some("fe".repeat(32)),
            _ => unreachable!("fixed test field"),
        }
        connection.execute(
            "UPDATE runs SET record=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_vec(&record)?, prepared.run_id],
        )?;
        let args = Args {
            command: Command::Verify(prepared.run_id.clone()),
            json: true,
            root: None,
        };
        assert!(
            commands::execute(args, workspace.root()).await.is_err(),
            "Prepared absence accepted substituted {field}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_verify_binds_the_record_to_historical_authority_request()
-> Result<(), Box<dyn Error>> {
    use attow_cli::{
        Workspace,
        args::{Args, Command},
        commands,
        demo::{self, DemoOptions, Scenario},
    };
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(dir.path())?;
    let mut record = demo::run(
        &workspace,
        DemoOptions {
            scenario: Scenario::DataChange,
            workers: 2,
            resume: None,
            browser: false,
            pause_after_prepared: false,
            pause_after_unknown: false,
            boundary_hook: None,
        },
    )
    .await?;
    let args = || Args {
        command: Command::Verify(record.run_id.clone()),
        json: true,
        root: None,
    };
    assert_eq!(
        commands::execute(args(), workspace.root()).await?.exit_code,
        0
    );
    record.request_id = Some("fe".repeat(32));
    let connection = rusqlite::Connection::open(workspace.data_dir().join("catalog.sqlite"))?;
    connection.execute(
        "UPDATE runs SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_vec(&record)?, record.run_id],
    )?;
    assert!(
        commands::execute(args(), workspace.root()).await.is_err(),
        "native verify accepted a substituted request ID"
    );
    Ok(())
}
