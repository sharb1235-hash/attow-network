//! Real admitted WASM, exact reuse, durable result objects and offline replay.
#[path = "../tests/support/scheduled_wasm.rs"]
mod scheduled_wasm;
use std::sync::atomic::Ordering;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let setup = scheduled_wasm::setup(&directory.path().join("offline.sqlite"))?;
    let run = setup.scheduler.submit(setup.graph)?;
    let report = setup.scheduler.drive(run).await?;
    if report.outcomes.len() != 2
        || !report.failures.is_empty()
        || setup.calls.load(Ordering::SeqCst) != 1
    {
        return Err("expected two independent outcomes from one real WASM evaluation".into());
    }
    let inputs = setup.repository.recorded_inputs(&report.trace)?;
    drop(setup.scheduler);
    drop(setup.repository);
    let replay = attow_network::scheduler::replay(&report.trace, &inputs)?;
    if replay.outcomes != report.outcomes || setup.calls.load(Ordering::SeqCst) != 1 {
        return Err("offline replay mismatch".into());
    }
    println!(
        "{}",
        serde_json::json!({"schema":1,"project":"Attow Network","backend":"real-wasmtime",
        "branches":2,"worker_evaluations":1,"replayed_outcomes":replay.outcomes.len(),
        "replay_worker_calls":0,"external_provider_calls":0,"output_hash":replay.output_hash.to_string()})
    );
    Ok(())
}
