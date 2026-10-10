//! Explicit checkpoint resume across a new native process.
use attow_network::sandbox::{
    ExecutionProfile, Executor, SqliteSandboxRepository, StepExit, StepInput,
};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn component() -> Result<Vec<u8>, wat::Error> {
    wat::parse_bytes(include_bytes!("../fixtures/original/wasm/continuation.wat"))
        .map(|bytes| bytes.into_owned())
}

fn executor(path: &Path) -> Result<Executor, attow_network::sandbox::SandboxError> {
    Executor::new(Arc::new(SqliteSandboxRepository::open(path)?))
}

#[test]
fn resume_child_entry() -> TestResult {
    let Ok(database) = std::env::var("ATTOW_SANDBOX_RESUME_DB") else {
        return Ok(());
    };
    let executor = executor(Path::new(&database))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code_id = std::env::var("ATTOW_SANDBOX_CODE")?.parse()?;
    let code = executor.load_code(code_id)?;
    assert_eq!(code.id(), code_id);
    let input = StepInput {
        code: code_id,
        state: std::env::var("ATTOW_SANDBOX_STATE")?.parse()?,
        continuation: std::env::var("ATTOW_SANDBOX_CONTINUATION")?.parse()?,
        observation: Some(std::env::var("ATTOW_SANDBOX_OBSERVATION")?.parse()?),
    };
    let StepExit::Done { result, delta } = executor.run_step(&code, input, &profile)? else {
        return Err("fresh child should resume to completion".into());
    };
    assert_eq!(executor.read_data(result)?, [42]);
    assert!(executor.read_data(delta)?.is_empty());
    fs::write(
        std::env::var("ATTOW_SANDBOX_RESULT")?,
        format!("{}\n{result}\n", std::process::id()),
    )?;
    Ok(())
}

#[test]
fn checkpoint_resumes_after_original_executor_is_destroyed_in_a_new_process() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("continuation.db");
    let output = directory.path().join("child-result.txt");
    let original = executor(&database)?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = original.admit(&component()?)?;
    let state = original.store_data(&[10])?;
    let initial = original.initial_continuation(&code, &profile, &[])?;
    let StepExit::Suspended {
        continuation,
        request,
        delta,
    } = original.run_step(
        &code,
        StepInput {
            code: code.id(),
            state,
            continuation: initial,
            observation: None,
        },
        &profile,
    )?
    else {
        return Err("original step should persist a suspended continuation".into());
    };
    assert_eq!(original.read_data(request)?, [99]);
    assert_eq!(original.read_data(delta)?, [3]);
    let observation = original.store_data(&[25])?;
    let code_id = code.id();
    drop(code);
    drop(original);

    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "resume_child_entry", "--nocapture"])
        .env("ATTOW_SANDBOX_RESUME_DB", &database)
        .env("ATTOW_SANDBOX_CODE", code_id.to_string())
        .env("ATTOW_SANDBOX_STATE", state.to_string())
        .env("ATTOW_SANDBOX_CONTINUATION", continuation.to_string())
        .env("ATTOW_SANDBOX_OBSERVATION", observation.to_string())
        .env("ATTOW_SANDBOX_RESULT", &output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(30) {
            child.kill()?;
            child.wait()?;
            return Err("new-process continuation resume exceeded bounded watchdog".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "resume child failed: {status}");
    let result_record = fs::read_to_string(output)?;
    let mut lines = result_record.lines();
    let child_pid: u32 = lines.next().ok_or("missing child PID")?.parse()?;
    assert_ne!(child_pid, std::process::id());
    let result = lines.next().ok_or("missing result hash")?.parse()?;
    // The child has exited; a third repository instance verifies durable output.
    assert_eq!(executor(&database)?.read_data(result)?, [42]);
    Ok(())
}
