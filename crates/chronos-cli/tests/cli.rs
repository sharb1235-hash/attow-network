use attow_cli::args::{Args, Command};
use attow_cli::{CliError, RunRecord, Workspace};
use std::error::Error;

#[test]
fn parser_rejects_ambiguous_or_unsafe_flags() {
    for args in [
        vec!["run", "checkout", "--workers", "0"],
        vec!["run", "checkout", "--workers", "2", "--workers", "3"],
        vec!["promote", "00", "--force"],
        vec!["replay", "00"],
        vec!["import", "bundle"],
        vec!["init", "--json"],
    ] {
        assert!(Args::parse(args.into_iter().map(Into::into)).is_err());
    }
}

#[test]
fn parser_supports_space_paths_and_explicit_offline_replay() -> Result<(), Box<dyn Error>> {
    let parsed = Args::parse(
        ["init", "owned workspace", "--json"]
            .into_iter()
            .map(Into::into),
    )?;
    assert!(parsed.json);
    assert_eq!(parsed.command, Command::Init("owned workspace".into()));
    let id = "12".repeat(32);
    let parsed = Args::parse(
        ["replay", &id, "--offline", "--json"]
            .into_iter()
            .map(Into::into),
    )?;
    assert_eq!(parsed.command, Command::Replay(id));
    Ok(())
}

#[test]
fn workspace_roundtrip_preserves_identity_and_rejects_reinitialization()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("owned workspace with spaces");
    let workspace = Workspace::init(&root)?;
    let writer = workspace.writer_id()?;
    assert!(workspace.authority_path().is_file());
    assert!(Workspace::init(&root).is_err());
    let reopened = Workspace::open(&root)?;
    assert_eq!(writer, reopened.writer_id()?);
    let record = RunRecord::new("11".repeat(32), "data-change".into());
    reopened.save_run(&record)?;
    assert_eq!(reopened.load_run(&record.run_id)?, record);
    assert!(reopened.is_native(&record.run_id)?);
    Ok(())
}

#[test]
fn untrusted_run_ids_never_become_paths() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let workspace = Workspace::init(temp.path())?;
    for id in ["../outside", "C:\\outside", "", &"A".repeat(64)] {
        assert!(matches!(workspace.load_run(id), Err(CliError::Input(_))));
        let record = RunRecord::new(id.into(), "data-change".into());
        assert!(matches!(
            workspace.save_run(&record),
            Err(CliError::Input(_))
        ));
    }
    assert!(workspace.list_runs()?.is_empty());
    Ok(())
}

#[test]
fn malformed_workspace_does_not_create_authority() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    assert!(Workspace::open(temp.path()).is_err());
    assert!(!temp.path().join(".attow").exists());
    Ok(())
}
