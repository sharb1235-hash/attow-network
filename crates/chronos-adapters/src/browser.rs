//! Bounded bridge to the pinned, original owned fixture. No live credentials.
use crate::{
    Adapter, AdapterCapabilities, AdapterError, AdapterLimits, AdapterManifest, AdapterStore,
    Fidelity, Observation, ObservationRequest, StorageSupport, WorkerBinding,
};
use chronos_codec::{Canonical, Object, ObjectId, Value};
use chronos_state::execution::{
    EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest,
    WorkerBackend,
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{ChildStdin, ChildStdout},
};

const FRAME: usize = 512 * 1024;
const SNAPSHOT: usize = 256 * 1024;
const DEADLINE: Duration = Duration::from_secs(10);
const SIDECAR: &[u8] = include_bytes!("../../../demos/workflow-studio/browser/sidecar.mjs");
const HTML: &[u8] = include_bytes!("../../../demos/workflow-studio/app/index.html");
const APP: &[u8] = include_bytes!("../../../demos/workflow-studio/app/app.js");
static RESIDENT: AtomicUsize = AtomicUsize::new(0);

/// Logical bridge buffers, not a limit on Chromium's native RSS or CPU.
pub const BROWSER_BUDGET: ResourceBudget = ResourceBudget {
    fuel: 1,
    memory_bytes: 1_048_576,
    output_bytes: SNAPSHOT as u64,
    host_calls: 1,
};
#[derive(Clone, Debug)]
pub struct BrowserConfig {
    pub node: PathBuf,
    pub sidecar: PathBuf,
    pub mode: Fidelity,
    pub credentials: bool,
}
impl BrowserConfig {
    pub fn new(node: PathBuf, sidecar: PathBuf) -> Self {
        Self {
            node,
            sidecar,
            mode: Fidelity::OwnedEnvironment,
            credentials: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct BrowserIdentities {
    pub code: ObjectId,
    pub profile: ObjectId,
    pub fidelity: ObjectId,
}
#[derive(Clone, Copy, Debug)]
pub enum StorageKind {
    Cookies,
    LocalStorage,
    IndexedDb,
    SessionStorage,
}
impl StorageKind {
    fn name(self) -> &'static str {
        match self {
            Self::Cookies => "cookies",
            Self::LocalStorage => "local",
            Self::IndexedDb => "database",
            Self::SessionStorage => "session",
        }
    }
}

struct Resident;
impl Resident {
    fn reserve() -> Result<Self, AdapterError> {
        RESIDENT
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map(|_| Self)
            .map_err(|_| AdapterError::Limit("resident browsers"))
    }
}
impl Drop for Resident {
    fn drop(&mut self) {
        RESIDENT.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Job {
    method: String,
    args: Json,
    reply: mpsc::SyncSender<Result<Json, AdapterError>>,
}
struct Bridge {
    sender: Option<mpsc::SyncSender<Job>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Bridge {
    fn start(config: BrowserConfig) -> Result<Self, AdapterError> {
        let resident = Resident::reserve()?;
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let handle = thread::Builder::new()
            .name("attow-browser".into())
            .spawn(move || {
                let _resident = resident;
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                let mut command = tokio::process::Command::new(&config.node);
                command
                    .arg(&config.sidecar)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .env_clear();
                // Only runtime paths survive; Node injection options and credentials do not.
                for key in [
                    "SystemRoot",
                    "WINDIR",
                    "TEMP",
                    "TMP",
                    "TMPDIR",
                    "LOCALAPPDATA",
                    "HOME",
                ] {
                    if let Some(value) = std::env::var_os(key) {
                        command.env(key, value);
                    }
                }
                if let Some(parent) = config.sidecar.parent() {
                    command
                        .current_dir(parent)
                        .env("PLAYWRIGHT_BROWSERS_PATH", parent.join(".browsers"));
                }
                let _entered = runtime.enter();
                let Ok(mut child) = command.spawn() else {
                    return;
                };
                let Some(mut input) = child.stdin.take() else {
                    return;
                };
                let Some(mut output) = child.stdout.take() else {
                    return;
                };
                let mut next = 1u32;
                while let Ok(job) = receiver.recv() {
                    let result = runtime.block_on(exchange(
                        &mut input,
                        &mut output,
                        next,
                        &job.method,
                        job.args,
                    ));
                    let failed = result.is_err();
                    let _ = job.reply.send(result);
                    let Some(id) = next.checked_add(1) else {
                        break;
                    };
                    next = id;
                    // A failed request retires the process: never reuse a desynchronized stream.
                    if failed {
                        break;
                    }
                }
                runtime.block_on(async {
                    let _ = exchange(&mut input, &mut output, next, "close", json!({})).await;
                    drop(input);
                    if tokio::time::timeout(Duration::from_secs(2), child.wait())
                        .await
                        .is_err()
                    {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                    }
                });
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(handle),
        })
    }
    fn call(&self, method: &str, args: Json) -> Result<Json, AdapterError> {
        let (reply, receive) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .ok_or(AdapterError::BackendUnavailable)?
            .try_send(Job {
                method: method.into(),
                args,
                reply,
            })
            .map_err(|_| AdapterError::BackendUnavailable)?;
        receive
            .recv_timeout(DEADLINE + Duration::from_secs(1))
            .map_err(|_| AdapterError::BackendUnavailable)?
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
async fn exchange(
    input: &mut ChildStdin,
    output: &mut ChildStdout,
    id: u32,
    method: &str,
    args: Json,
) -> Result<Json, AdapterError> {
    let bytes = serde_json::to_vec(&json!({"version":1,"id":id,"method":method,"args":args}))
        .map_err(|_| AdapterError::Protocol)?;
    if bytes.len() > FRAME {
        return Err(AdapterError::Limit("bridge frame"));
    }
    tokio::time::timeout(DEADLINE, async {
        input.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        input.write_all(&bytes).await?;
        input.flush().await?;
        let size = output.read_u32().await? as usize;
        if size == 0 || size > FRAME {
            return Err(AdapterError::Limit("bridge frame"));
        }
        let mut bytes = vec![0; size];
        output.read_exact(&mut bytes).await?;
        let response: Json = serde_json::from_slice(&bytes).map_err(|_| AdapterError::Protocol)?;
        let fields = response.as_object().ok_or(AdapterError::Protocol)?;
        if response["version"] != 1 || response["id"] != id || fields.len() != 4 {
            return Err(AdapterError::Protocol);
        }
        match response["ok"].as_bool() {
            Some(true) if fields.contains_key("result") => Ok(response["result"].clone()),
            Some(false) if response["error"] == "unsupported-or-invalid-request" => {
                Err(AdapterError::InvalidRequest)
            }
            _ => Err(AdapterError::Protocol),
        }
    })
    .await
    .map_err(|_| AdapterError::Timeout)?
}

pub struct BrowserAdapter {
    store: AdapterStore,
    binding: WorkerBinding,
    bridge: Mutex<Bridge>,
    delta: ObjectId,
}
impl BrowserAdapter {
    pub fn identities(
        store: &AdapterStore,
        owner: &str,
    ) -> Result<BrowserIdentities, AdapterError> {
        let code = store.put_object(
            owner,
            Value::Array(vec![
                Value::Text("attow-owned-browser-code".into()),
                Value::Unsigned(1),
                Value::Bytes(blake3::hash(SIDECAR).as_bytes().to_vec()),
                Value::Bytes(blake3::hash(HTML).as_bytes().to_vec()),
                Value::Bytes(blake3::hash(APP).as_bytes().to_vec()),
            ]),
            BTreeSet::new(),
        )?;
        let profile = store.put_object(owner, Value::Array(vec![Value::Text("attow-owned-browser-profile".into()), Value::Unsigned(1),
            Value::Text("Node24.21.0/Playwright1.64.0/Chromium156.0.8078.4/revision1248; fixture-only; no credentials; storage256KiB/frame512KiB/deadline10s; buffers-not-RSS".into()),
            Value::Text(std::env::consts::OS.into()), Value::Text(std::env::consts::ARCH.into()), Value::Bytes(code.0.to_vec())]), BTreeSet::from([code]))?;
        let fidelity = store.put_object(
            owner,
            Value::Array(vec![
                Value::Text("attow-owned-browser-fidelity".into()),
                Value::Unsigned(1),
                Value::Text("OwnedEnvironment; checkout_preview=Model; never Pure".into()),
            ]),
            BTreeSet::new(),
        )?;
        Ok(BrowserIdentities {
            code,
            profile,
            fidelity,
        })
    }
    pub fn manifest() -> AdapterManifest {
        AdapterManifest {
            version: 1,
            name: "attow-owned-browser".into(),
            fidelity: Fidelity::OwnedEnvironment,
            storage: StorageSupport {
                cookies: true,
                local_storage: true,
                indexed_db: true,
                session_storage: true,
                hidden_stack: false,
            },
            deterministic_subset: vec![],
            effect_semantics: vec![
                "owned fixture observations only; checkout preview is Model".into(),
            ],
            limits: AdapterLimits {
                snapshot_bytes: SNAPSHOT,
                output_bytes: SNAPSHOT,
                deadline_ms: 10_000,
            },
        }
    }
    pub fn launch(
        store: AdapterStore,
        binding: WorkerBinding,
        mut config: BrowserConfig,
    ) -> Result<Self, AdapterError> {
        if config.mode != Fidelity::OwnedEnvironment
            || config.credentials
            || binding.effect_class != EffectClass::FreshObservation
            || binding.budget != BROWSER_BUDGET
        {
            return Err(AdapterError::Unsupported);
        }
        let identities = Self::identities(&store, "browser:identities")?;
        if binding.code != identities.code
            || binding.profile != identities.profile
            || binding.fidelity != identities.fidelity
        {
            return Err(AdapterError::Unsupported);
        }
        config.node = config.node.canonicalize()?;
        config.sidecar = config.sidecar.canonicalize()?;
        let parent = config
            .sidecar
            .parent()
            .ok_or(AdapterError::InvalidRequest)?;
        for (path, expected) in [
            (config.sidecar.clone(), SIDECAR),
            (parent.join("../app/index.html"), HTML),
            (parent.join("../app/app.js"), APP),
        ] {
            verify_source(&path, expected)?;
        }
        let bridge = Bridge::start(config)?;
        if bridge.call("hello", json!({}))?
            != json!({"node":"24.21.0","playwright":"1.64.0","chromium":"156.0.8078.4","revision":"1248","mode":"OwnedEnvironment","credentials":false})
        {
            return Err(AdapterError::Unsupported);
        }
        let delta = store.put_object("browser:empty-delta", Value::Null, BTreeSet::new())?;
        Ok(Self {
            store,
            binding,
            bridge: Mutex::new(bridge),
            delta,
        })
    }
    fn call(&self, method: &str, args: Json) -> Result<Json, AdapterError> {
        self.bridge
            .try_lock()
            .map_err(|_| AdapterError::BackendUnavailable)?
            .call(method, args)
    }
    pub fn set_storage(
        &mut self,
        kind: StorageKind,
        key: &str,
        value: &str,
    ) -> Result<(), AdapterError> {
        storage_pair(key, value)?;
        if self.call(
            "storage_set",
            json!({"kind":kind.name(),"key":key,"value":value}),
        )? != json!({"stored":true})
        {
            return Err(AdapterError::Protocol);
        }
        Ok(())
    }
    pub fn read_storage(
        &mut self,
        kind: StorageKind,
        key: &str,
    ) -> Result<Option<String>, AdapterError> {
        storage_pair(key, "")?;
        let result = self.call("storage_get", json!({"kind":kind.name(),"key":key}))?;
        if result.is_null() {
            Ok(None)
        } else {
            result
                .as_str()
                .map(|s| Some(s.to_owned()))
                .ok_or(AdapterError::Protocol)
        }
    }
    pub fn snapshot(&mut self) -> Result<ObjectId, AdapterError> {
        let bytes = serde_json::to_vec(&self.call("snapshot", json!({}))?)
            .map_err(|_| AdapterError::Protocol)?;
        if bytes.len() > SNAPSHOT {
            return Err(AdapterError::Limit("snapshot"));
        }
        self.store.put_object(
            &format!("browser:snapshot:{}", self.binding.security_scope),
            Value::Array(vec![
                Value::Text("attow-browser-snapshot".into()),
                Value::Unsigned(1),
                Value::Bytes(self.binding.tenant.0.to_vec()),
                Value::Bytes(self.binding.security_scope.0.to_vec()),
                Value::Bytes(self.binding.profile.0.to_vec()),
                Value::Bytes(bytes),
            ]),
            BTreeSet::from([self.binding.profile]),
        )
    }
    fn snapshot_data(&self, id: ObjectId) -> Result<Json, AdapterError> {
        let object = self.store.get_object(id)?;
        let Value::Array(fields) = object.payload else {
            return Err(AdapterError::InvalidSnapshot);
        };
        let [
            Value::Text(tag),
            Value::Unsigned(1),
            Value::Bytes(tenant),
            Value::Bytes(scope),
            Value::Bytes(profile),
            Value::Bytes(bytes),
        ] = fields.as_slice()
        else {
            return Err(AdapterError::InvalidSnapshot);
        };
        if tag != "attow-browser-snapshot"
            || bytes.len() > SNAPSHOT
            || profile != &self.binding.profile.0
            || object.refs != BTreeSet::from([self.binding.profile])
        {
            return Err(AdapterError::InvalidSnapshot);
        }
        if tenant != &self.binding.tenant.0 || scope != &self.binding.security_scope.0 {
            return Err(AdapterError::OwnerMismatch);
        }
        serde_json::from_slice(bytes).map_err(|_| AdapterError::InvalidSnapshot)
    }
    pub fn restore(&mut self, id: ObjectId) -> Result<(), AdapterError> {
        let snapshot = self.snapshot_data(id)?;
        if self.call("restore", json!({"snapshot":snapshot}))? != json!({"restored":true}) {
            return Err(AdapterError::Protocol);
        }
        Ok(())
    }
    pub fn reject_hidden_stack(&mut self) -> Result<(), AdapterError> {
        Err(AdapterError::Unsupported)
    }
    pub fn network_probe(&mut self) -> Result<Json, AdapterError> {
        self.call("network_probe", json!({}))
    }
    /// Only two argument-free observation methods are admitted. No arbitrary JS or URL.
    pub fn store_request(
        store: &AdapterStore,
        owner: &str,
        method: &str,
    ) -> Result<ObjectId, AdapterError> {
        validate_method(method)?;
        store.put_object(
            owner,
            Value::Array(vec![
                Value::Text("attow-browser-request".into()),
                Value::Unsigned(1),
                Value::Text(method.into()),
            ]),
            BTreeSet::new(),
        )
    }
    fn request_method(&self, id: ObjectId) -> Result<String, AdapterError> {
        let object = self.store.get_object(id)?;
        let Value::Array(fields) = object.payload else {
            return Err(AdapterError::InvalidRequest);
        };
        let [Value::Text(tag), Value::Unsigned(1), Value::Text(method)] = fields.as_slice() else {
            return Err(AdapterError::InvalidRequest);
        };
        if tag != "attow-browser-request" || !object.refs.is_empty() {
            return Err(AdapterError::InvalidRequest);
        }
        validate_method(method)?;
        Ok(method.clone())
    }
    fn retain_observation(
        &self,
        method: &str,
        data: Json,
        mut refs: BTreeSet<ObjectId>,
    ) -> Result<Observation, AdapterError> {
        let bytes = serde_json::to_vec(&data).map_err(|_| AdapterError::Protocol)?;
        if bytes.len() > SNAPSHOT {
            return Err(AdapterError::Limit("observation"));
        }
        let fidelity = if method == "checkout_preview" {
            Fidelity::Model
        } else {
            Fidelity::OwnedEnvironment
        };
        let captured_at_unix_ms = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| AdapterError::BackendUnavailable)?
                .as_millis(),
        )
        .map_err(|_| AdapterError::BackendUnavailable)?;
        refs.insert(self.binding.profile);
        refs.insert(self.binding.fidelity);
        let object = Object {
            payload: Value::Array(vec![
                Value::Text("attow-browser-observation".into()),
                Value::Unsigned(1),
                Value::Text(format!("{fidelity:?}")),
                Value::Unsigned(captured_at_unix_ms),
                Value::Bytes(self.binding.tenant.0.to_vec()),
                Value::Bytes(self.binding.security_scope.0.to_vec()),
                Value::Text(method.into()),
                Value::Bytes(bytes),
            ]),
            refs,
        };
        if object.encode()?.len() as u64 > self.binding.budget.output_bytes {
            return Err(AdapterError::Limit("observation"));
        }
        let root = self.store.put_object(
            &format!("browser:observations:{}", self.binding.security_scope),
            object.payload,
            object.refs,
        )?;
        Ok(Observation {
            root,
            provenance: self.binding.profile,
            fidelity,
            captured_at_unix_ms,
        })
    }
}
fn verify_source(path: &Path, expected: &[u8]) -> Result<(), AdapterError> {
    if std::fs::metadata(path)?.len() != expected.len() as u64 || std::fs::read(path)? != expected {
        return Err(AdapterError::Unsupported);
    }
    Ok(())
}
fn validate_method(method: &str) -> Result<(), AdapterError> {
    if matches!(method, "snapshot" | "checkout_preview") {
        Ok(())
    } else {
        Err(AdapterError::Unsupported)
    }
}
fn storage_pair(key: &str, value: &str) -> Result<(), AdapterError> {
    if key.is_empty() || key.len() > 128 || value.len() > 4096 {
        return Err(AdapterError::Limit("storage entry"));
    }
    Ok(())
}
impl Adapter for BrowserAdapter {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            fidelity: Fidelity::OwnedEnvironment,
            snapshot: true,
            restore: true,
            ..AdapterCapabilities::default()
        }
    }
    fn snapshot(&mut self) -> Result<ObjectId, AdapterError> {
        Self::snapshot(self)
    }
    fn restore(&mut self, root: ObjectId) -> Result<(), AdapterError> {
        Self::restore(self, root)
    }
    fn observe(&mut self, request: ObservationRequest) -> Result<Observation, AdapterError> {
        if request.input.is_some() {
            return Err(AdapterError::Unsupported);
        }
        validate_method(&request.method)?;
        let data = self.call(&request.method, json!({}))?;
        self.retain_observation(&request.method, data, BTreeSet::new())
    }
}
fn execution_error(error: AdapterError) -> ExecutionError {
    match error {
        AdapterError::Limit(_) => ExecutionError::ResourceExhausted,
        AdapterError::BackendUnavailable
        | AdapterError::Storage
        | AdapterError::Io(_)
        | AdapterError::Timeout => ExecutionError::BackendUnavailable,
        _ => ExecutionError::InvalidRequest,
    }
}
impl WorkerBackend for BrowserAdapter {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Browser
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.binding.validate(request, ResourceClass::Browser)?;
        self.snapshot_data(request.input_root)
            .map_err(execution_error)?;
        self.request_method(request.continuation)
            .map_err(execution_error)?;
        Ok(())
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        self.validate_request(&request)?;
        let method = self
            .request_method(request.continuation)
            .map_err(execution_error)?;
        let snapshot = self
            .snapshot_data(request.input_root)
            .map_err(execution_error)?;
        // Hold this one lease through restore and observe: concurrent branches cannot interleave.
        let bridge = self
            .bridge
            .try_lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        if bridge
            .call("restore", json!({"snapshot":snapshot}))
            .map_err(execution_error)?
            != json!({"restored":true})
        {
            return Err(ExecutionError::BackendUnavailable);
        }
        let data = bridge.call(&method, json!({})).map_err(execution_error)?;
        let observation = self
            .retain_observation(
                &method,
                data,
                BTreeSet::from([
                    request.input_root,
                    request.continuation,
                    request.observation_root,
                ]),
            )
            .map_err(execution_error)?;
        Ok(StepOutcome {
            attempt: request.attempt,
            profile: request.profile,
            exit: StepExit::Completed {
                result: observation.root,
                delta: self.delta,
                observation_root: observation.root,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_limit_includes_its_canonical_envelope() -> Result<(), AdapterError> {
        let dir = tempfile::tempdir()?;
        let store = AdapterStore::open(&dir.path().join("observations.db"))?;
        let ids = BrowserAdapter::identities(&store, "test:ids")?;
        let owner = ObjectId([1; 32]);
        let delta = store.put_object("test:delta", Value::Null, BTreeSet::new())?;
        let browser = BrowserAdapter {
            store,
            delta,
            binding: WorkerBinding {
                tenant: owner,
                security_scope: owner,
                policy: ids.profile,
                profile: ids.profile,
                code: ids.code,
                fidelity: ids.fidelity,
                effect_class: EffectClass::FreshObservation,
                budget: BROWSER_BUDGET,
            },
            bridge: Mutex::new(Bridge {
                sender: None,
                thread: None,
            }),
        };
        let data = Json::String("x".repeat(SNAPSHOT - 2));
        assert_eq!(
            serde_json::to_vec(&data).expect("JSON string").len(),
            SNAPSHOT
        );
        assert!(matches!(
            browser.retain_observation("snapshot", data, BTreeSet::new()),
            Err(AdapterError::Limit("observation"))
        ));
        Ok(())
    }
}
