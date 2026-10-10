#![cfg(feature = "browser")]
use attow_adapters::{
    AdapterError, AdapterStore, Fidelity, WorkerBinding,
    browser::{BROWSER_BUDGET, BrowserAdapter, BrowserConfig, StorageKind},
};
use chronos_codec::ObjectId;
use chronos_state::execution::EffectClass;
use std::{path::PathBuf, sync::Mutex};
static SERIAL: Mutex<()> = Mutex::new(());
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn config() -> BrowserConfig {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node = std::env::var_os("ATTOW_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".tools/node-v24.21.0-win-x64/node.exe"));
    BrowserConfig::new(node, root.join("demos/workflow-studio/browser/sidecar.mjs"))
}
fn binding(store: &AdapterStore, owner: u8) -> Result<WorkerBinding, AdapterError> {
    let ids = BrowserAdapter::identities(store, "browser-test-identities")?;
    Ok(WorkerBinding {
        tenant: ObjectId([owner; 32]),
        security_scope: ObjectId([owner; 32]),
        policy: ids.profile,
        fidelity: ids.fidelity,
        code: ids.code,
        profile: ids.profile,
        effect_class: EffectClass::FreshObservation,
        budget: BROWSER_BUDGET,
    })
}
fn storage_roundtrip(kind: StorageKind) -> TestResult {
    let _serial = SERIAL.lock().map_err(|_| "test lock")?;
    let dir = tempfile::tempdir()?;
    let store = AdapterStore::open(&dir.path().join("browser.db"))?;
    let admission = binding(&store, 1)?;
    let mut first = BrowserAdapter::launch(store.clone(), admission.clone(), config())?;
    first.set_storage(kind, "fixture", "original")?;
    let snapshot = first.snapshot()?;
    first.set_storage(kind, "fixture", "changed")?;
    assert_eq!(first.read_storage(kind, "fixture")?, Some("changed".into()));
    first.restore(snapshot)?;
    assert_eq!(
        first.read_storage(kind, "fixture")?,
        Some("original".into())
    );
    drop(first);
    let mut reopened = BrowserAdapter::launch(store, admission, config())?;
    reopened.restore(snapshot)?;
    assert_eq!(
        reopened.read_storage(kind, "fixture")?,
        Some("original".into())
    );
    Ok(())
}
#[test]
fn cookies_restore_across_context_and_process() -> TestResult {
    storage_roundtrip(StorageKind::Cookies)
}
#[test]
fn local_storage_restores_across_context_and_process() -> TestResult {
    storage_roundtrip(StorageKind::LocalStorage)
}
#[test]
fn indexed_db_restores_across_context_and_process() -> TestResult {
    storage_roundtrip(StorageKind::IndexedDb)
}
#[test]
fn session_storage_restores_explicitly_across_context_and_process() -> TestResult {
    storage_roundtrip(StorageKind::SessionStorage)
}
#[test]
fn contexts_are_separate_and_snapshot_owner_cannot_change() -> TestResult {
    let _serial = SERIAL.lock().map_err(|_| "test lock")?;
    let dir = tempfile::tempdir()?;
    let store = AdapterStore::open(&dir.path().join("browser.db"))?;
    let mut first = BrowserAdapter::launch(store.clone(), binding(&store, 1)?, config())?;
    let mut sibling = BrowserAdapter::launch(store.clone(), binding(&store, 1)?, config())?;
    first.set_storage(StorageKind::LocalStorage, "private", "first")?;
    assert_eq!(
        sibling.read_storage(StorageKind::LocalStorage, "private")?,
        None
    );
    let snapshot = first.snapshot()?;
    let mut other = BrowserAdapter::launch(store.clone(), binding(&store, 2)?, config())?;
    assert!(matches!(
        other.restore(snapshot),
        Err(AdapterError::OwnerMismatch)
    ));
    sibling.restore(snapshot)?;
    sibling.set_storage(StorageKind::LocalStorage, "private", "sibling")?;
    assert_eq!(
        first.read_storage(StorageKind::LocalStorage, "private")?,
        Some("first".into())
    );
    Ok(())
}
#[test]
fn hidden_stack_live_credentials_and_false_purity_fail_closed() -> TestResult {
    let _serial = SERIAL.lock().map_err(|_| "test lock")?;
    let dir = tempfile::tempdir()?;
    let store = AdapterStore::open(&dir.path().join("browser.db"))?;
    let admission = binding(&store, 1)?;
    let mut live = config();
    live.mode = Fidelity::Live;
    assert!(matches!(
        BrowserAdapter::launch(store.clone(), admission.clone(), live),
        Err(AdapterError::Unsupported)
    ));
    let mut credentialed = config();
    credentialed.credentials = true;
    assert!(matches!(
        BrowserAdapter::launch(store.clone(), admission.clone(), credentialed),
        Err(AdapterError::Unsupported)
    ));
    let mut pure = admission.clone();
    pure.effect_class = EffectClass::Pure;
    assert!(matches!(
        BrowserAdapter::launch(store.clone(), pure, config()),
        Err(AdapterError::Unsupported)
    ));
    let mut browser = BrowserAdapter::launch(store, admission, config())?;
    assert!(matches!(
        browser.reject_hidden_stack(),
        Err(AdapterError::Unsupported)
    ));
    Ok(())
}
#[test]
fn forbidden_loopback_server_receives_no_fetch_websocket_or_service_worker_request() -> TestResult {
    let _serial = SERIAL.lock().map_err(|_| "test lock")?;
    let dir = tempfile::tempdir()?;
    let store = AdapterStore::open(&dir.path().join("browser.db"))?;
    let mut browser = BrowserAdapter::launch(store.clone(), binding(&store, 1)?, config())?;
    let report = browser.network_probe()?;
    assert_eq!(report["forbidden_requests"], 0);
    assert_eq!(report["fetch_blocked"], true);
    assert_eq!(report["websocket_blocked"], true);
    assert_eq!(report["service_worker_blocked"], true);
    assert_eq!(report["service_worker_requests"], 0);
    Ok(())
}

#[test]
fn resident_limit_releases_only_after_the_real_process_exits() -> TestResult {
    let _serial = SERIAL.lock().map_err(|_| "test lock")?;
    let dir = tempfile::tempdir()?;
    let store = AdapterStore::open(&dir.path().join("browsers.db"))?;
    let admission = binding(&store, 1)?;
    let mut residents = Vec::new();
    for _ in 0..4 {
        residents.push(BrowserAdapter::launch(
            store.clone(),
            admission.clone(),
            config(),
        )?);
    }
    assert!(matches!(
        BrowserAdapter::launch(store.clone(), admission.clone(), config()),
        Err(AdapterError::Limit("resident browsers"))
    ));
    drop(residents.pop());
    let mut replacement = BrowserAdapter::launch(store, admission, config())?;
    assert_eq!(
        replacement.read_storage(StorageKind::LocalStorage, "absent")?,
        None
    );
    Ok(())
}

#[test]
fn real_sidecar_rejects_oversized_frames_bad_ids_and_unknown_methods() -> TestResult {
    use std::process::Stdio;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _serial = SERIAL.lock().map_err(|_| "test lock")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        for case in 0..3 {
            let configuration = config();
            let mut child = tokio::process::Command::new(configuration.node)
                .arg(&configuration.sidecar)
                .env("PLAYWRIGHT_BROWSERS_PATH", configuration.sidecar.parent().ok_or("parent")?.join(".browsers"))
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
                .kill_on_drop(true).spawn()?;
            let mut input = child.stdin.take().ok_or("stdin")?;
            let mut output = child.stdout.take().ok_or("stdout")?;
            if case == 0 {
                input.write_all(&524_289u32.to_be_bytes()).await?;
            } else {
                let body = serde_json::to_vec(&serde_json::json!({"version":1,"id":if case==1 {2} else {1},"method":"arbitrary_eval","args":{"source":"fetch('https://example.com')"}}))?;
                input.write_all(&(body.len() as u32).to_be_bytes()).await?;
                input.write_all(&body).await?;
            }
            if case == 2 {
                let response = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    let length = output.read_u32().await?;
                    if length > 1024 { return Err(std::io::Error::other("unbounded error response")); }
                    let mut bytes = vec![0; length as usize]; output.read_exact(&mut bytes).await?;
                    Ok::<_, std::io::Error>(bytes)
                }).await??;
                let response: serde_json::Value = serde_json::from_slice(&response)?;
                assert_eq!(response["ok"], false);
                assert_eq!(response["error"], "unsupported-or-invalid-request");
            }
            drop(input);
            let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await??;
            if case < 2 { assert!(!status.success()); } else { assert!(status.success()); }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}
