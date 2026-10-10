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
