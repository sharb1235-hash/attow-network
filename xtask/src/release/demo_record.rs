//! Actual owned-fixture subprocess recording. No native workspace is exported.
use super::{ReleaseError, Result};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const COMMAND_BYTES: usize = 2 * 1024 * 1024;
const RECORDING_BYTES: usize = 16 * 1024 * 1024;

pub struct RuntimeOverrides {
    pub node: PathBuf,
    pub sidecar: PathBuf,
    pub browsers: PathBuf,
}
pub struct DemoRecording {
    pub artifacts: Vec<String>,
    pub metadata: Value,
}
impl RuntimeOverrides {
    pub fn from_env() -> Result<Self> {
        let path = |name| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .ok_or(ReleaseError::Invalid(
                    "recording requires explicit browser runtime paths",
                ))
        };
        let value = Self {
            node: path("ATTOW_NODE")?,
            sidecar: path("ATTOW_BROWSER_SIDECAR")?,
            browsers: path("PLAYWRIGHT_BROWSERS_PATH")?,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        for path in [&self.node, &self.sidecar, &self.browsers] {
            if !path.is_absolute() {
                return Err(ReleaseError::Invalid("runtime path must be absolute"));
            }
            super::manifest::reject_link(path)?;
        }
        if !self.node.is_file() || !self.sidecar.is_file() || !self.browsers.is_dir() {
            return Err(ReleaseError::Invalid("browser runtime is missing"));
        }
        Ok(())
    }
    fn apply(&self, command: &mut Command) {
        command
            .env("ATTOW_NODE", &self.node)
            .env("ATTOW_BROWSER_SIDECAR", &self.sidecar)
            .env("PLAYWRIGHT_BROWSERS_PATH", &self.browsers);
    }
}
pub fn record(root: &Path, output: &Path) -> Result<DemoRecording> {
    let runtime = RuntimeOverrides::from_env()?;
    let status = Command::new("cargo")
        .args([
            "+1.95.0",
            "build",
            "-p",
            "attow-cli",
            "--release",
            "--features",
            "browser",
            "--locked",
            "-j",
            "2",
        ])
        .current_dir(root)
        .status()?;
    if !status.success() {
        return Err(ReleaseError::Tool(
            "release browser CLI build failed".into(),
        ));
    }
    let metadata = Command::new("cargo")
        .args([
            "+1.95.0",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--locked",
        ])
        .current_dir(root)
        .output()?;
    if !metadata.status.success() {
        return Err(ReleaseError::Tool("Cargo metadata failed".into()));
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout)?;
    let target = metadata["target_directory"]
        .as_str()
        .ok_or(ReleaseError::Invalid("Cargo target directory"))?;
    let binary =
        Path::new(target)
            .join("release")
            .join(if cfg!(windows) { "attow.exe" } else { "attow" });
    record_with_binary(root, output, &binary, "release", &runtime)
}
pub fn record_with_binary(
    root: &Path,
    output: &Path,
    binary: &Path,
    profile: &str,
    runtime: &RuntimeOverrides,
) -> Result<DemoRecording> {
    if profile != "release" {
        return Err(ReleaseError::Invalid(
            "demo recording requires the release executable",
        ));
    }
    runtime.validate()?;
    let binary = fs::canonicalize(binary)?;
    super::manifest::reject_link(&binary)?;
    let executable = super::manifest::hash_file(&binary)?;
    let source = super::manifest::source_revision(root)?;
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        root.join(output)
    };
    fs::create_dir_all(&output)?;
    super::manifest::reject_link(&output)?;
    if fs::read_dir(&output)?.next().is_some() {
        return Err(ReleaseError::Invalid("recording output must be empty"));
    }
    // A fresh native authority, provider database and external fence live only
    // in this temporary directory, never in the distributable candidate.
    let workspace = tempfile::Builder::new()
        .prefix("attow recording ")
        .tempdir()?;
    let started_unix_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ReleaseError::Invalid("system clock before epoch"))?
            .as_millis(),
    )
    .map_err(|_| ReleaseError::Invalid("system clock overflow"))?;
    let mut recording = Recorder {
        start: Instant::now(),
        frames: vec![],
        commands: vec![],
        bytes: 0,
    };
    recording.note("Objective: inspect five alternative worldlines using actual WASM, an owned SQL snapshot and owned browser contexts, then authorize one simulated-provider operation. The configured worker limit is 2; observed concurrency is reported separately.\n")?;
    recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["init", ".", "--json"],
        0,
    )?;
    let doctor = recording.run(&binary, workspace.path(), runtime, &["doctor", "--json"], 0)?;
    if doctor["runtime_profile_hash"].as_str().is_none() || doctor["browser"]["compiled"] != true {
        return Err(ReleaseError::Invalid(
            "recorded binary lacks the required runtime profile or browser feature",
        ));
    }
    let prepared = recording.run(
        &binary,
        workspace.path(),
        runtime,
        &[
            "run",
            "checkout",
            "--workers",
            "2",
            "--browser",
            "--pause-after-prepared",
            "--json",
        ],
        0,
    )?;
    require(
        prepared["phase"] == "prepared" && prepared["intent"].is_null(),
        "expected unpromoted prepared checkout",
    )?;
    let checkout_id = id(&prepared, "run_id")?;
    let branches = prepared["branches"]
        .as_array()
        .ok_or(ReleaseError::Invalid("prepared branches"))?;
    require(
        branches.len() == 5,
        "expected five actual prepared branches",
    )?;
    let left = id(&branches[0], "branch_id")?;
    let right = id(&branches[1], "branch_id")?;
    recording.note(&format!("Prepared {} branches; configured worker limit 2, observed peak {}, logical nodes {}, executed {}, reused {}. Provider preview is a model, not confirmation.\n",branches.len(),metric(&prepared,"peak_resident")?,metric(&prepared,"logical_nodes")?,metric(&prepared,"executed")?,metric(&prepared,"reused")?))?;
    let listed = recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["branch", "list", "--json"],
        0,
    )?;
    require(
        listed["total_branches"] == 5,
        "prepared branch listing mismatch",
    )?;
    recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["compare", &left, &right, "--json"],
        0,
    )?;
    let verified = recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["verify", &checkout_id, "--json"],
        0,
    )?;
    require(
        verified["native_promotions"] == json!([]),
        "preparation already promoted",
    )?;
    recording.note("Select the prepared branch and transfer its graph between two private loopback peers. The durable intent authorizes a single operation at the simulated provider; its committed response is deliberately lost.\n")?;
    let unknown = recording.run(
        &binary,
        workspace.path(),
        runtime,
        &[
            "run",
            "checkout",
            "--workers",
            "2",
            "--browser",
            "--resume",
            &checkout_id,
            "--pause-after-unknown",
            "--json",
        ],
        3,
    )?;
    require(
        unknown["phase"] == "unknown"
            && unknown["run_id"] == prepared["run_id"]
            && unknown["operation"] == prepared["operation"],
        "Unknown did not preserve the prepared operation",
    )?;
    let intent = id(&unknown, "intent")?;
    let operation = id(&unknown, "operation")?;
    require(
        unknown["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|e| e["kind"] == "peer_transfer" && e["durable"] == true)
        }),
        "missing actual durable peer transfer",
    )?;
    recording.note(&format!("Unknown: intent {intent}, operation {operation}. A lost response is not rejection. Reconcile this same provider identity without another dispatch.\n"))?;
    let checkout = recording.run(
        &binary,
        workspace.path(),
        runtime,
        &[
            "run",
            "checkout",
            "--workers",
            "2",
            "--browser",
            "--resume",
            &checkout_id,
            "--json",
        ],
        0,
    )?;
    require(
        checkout["phase"] == "completed"
            && checkout["intent"] == unknown["intent"]
            && checkout["operation"] == unknown["operation"],
        "reconciliation changed identity or did not complete",
    )?;
    require(
        checkout["summary"]["provider"]["accepted"] == 1
            && checkout["summary"]["provider"]["dispatch_requests"] == 1,
        "provider did not accept exactly one dispatch",
    )?;
    require(
        checkout["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|e| e["kind"] == "provider_reconciliation" && e["state"] == "Confirmed")
        }),
        "missing Confirmed reconciliation",
    )?;
    recording.note("Confirmed by the owned simulator receipt. The dependent response was rerun against that receipt. Offline replay below has no worker or dispatch backend.\n")?;
    recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["replay", &checkout_id, "--offline", "--json"],
        0,
    )?;
    recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["verify", &checkout_id, "--json"],
        0,
    )?;
    recording.note("Noncommerce example: five alternatives for an owned sensor sampling setting, using shared WASM and immutable SQL observations.\n")?;
    let data_change = recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["run", "data-change", "--workers", "2", "--json"],
        0,
    )?;
    require(
        data_change["phase"] == "completed" && data_change["intent"].is_null(),
        "data-change must complete without a provider intent",
    )?;
    let data_id = id(&data_change, "run_id")?;
    recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["replay", &data_id, "--offline", "--json"],
        0,
    )?;
    recording.run(
        &binary,
        workspace.path(),
        runtime,
        &["verify", &data_id, "--json"],
        0,
    )?;
    recording.note(&format!("Data-change: configured worker limit 2, observed peak {}, logical nodes {}, executed {}, reused {}. Recorded all command output at its observed time; no speedup claim.\n",metric(&data_change,"peak_resident")?,metric(&data_change,"logical_nodes")?,metric(&data_change,"executed")?,metric(&data_change,"reused")?))?;
    require(
        super::manifest::hash_file(&binary)? == executable,
        "executable changed during recording",
    )?;
    require(
        super::manifest::source_revision(root)? == source,
        "source revision or dirty state changed during recording",
    )?;
    let elapsed_ms = millis(recording.start);
    let count = recording.commands.len();
    let events = json!({"schema":1,"fixture_mode":"owned","started_unix_ms":started_unix_ms,"commands":recording.commands,"runs":[checkout,data_change],"frames":recording.frames});
    let raw = serde_json::to_vec_pretty(&events)?;
    require(
        raw.len() <= RECORDING_BYTES,
        "recording exceeds its total byte limit",
    )?;
    write(&output.join("events.json"), &raw)?;
    write(&output.join("terminal.cast"), &cast(&events)?)?;
    write(&output.join("index.html"), viewer(&events)?.as_bytes())?;
    let files = ["events.json", "index.html", "terminal.cast"]
        .into_iter()
        .map(|path| {
            let (bytes, sha256) = super::manifest::hash_file(&output.join(path))?;
            Ok(json!({"path":path,"bytes":bytes,"sha256":sha256}))
        })
        .collect::<Result<Vec<_>>>()?;
    let summary = |run: &Value| -> Result<Value> {
        Ok(
            json!({"run_id":id(run,"run_id")?,"configured_worker_limit":2,"observed_peak":metric(run,"peak_resident")?,"logical_nodes":metric(run,"logical_nodes")?,"executed":metric(run,"executed")?,"reused":metric(run,"reused")?}),
        )
    };
    let mut checkout_summary = summary(&events["runs"][0])?;
    checkout_summary["operation"] = json!(operation);
    checkout_summary["intent"] = json!(intent);
    checkout_summary["prepared_branches"] = json!(branches.len());
    let metadata = json!({"schema":1,"source":source,"executable":{"bytes":executable.0,"sha256":executable.1,"profile":profile},"started_unix_ms":started_unix_ms,"elapsed_ms":elapsed_ms,"fixture_mode":"owned","provider":"simulated","files":files,"commands":count,"checkout":checkout_summary,"data_change":summary(&events["runs"][1])?});
    super::manifest::write_json(&output.join("recording.json"), &metadata)?;
    Ok(DemoRecording {
        artifacts: vec![
            "events.json".into(),
            "index.html".into(),
            "recording.json".into(),
            "terminal.cast".into(),
        ],
        metadata,
    })
}
fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(ReleaseError::Invalid(message))
    }
}
fn id(value: &Value, field: &str) -> Result<String> {
    let id = value[field]
        .as_str()
        .ok_or(ReleaseError::Invalid("recorded identity is missing"))?;
    require(
        id.len() == 64
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "recorded identity is invalid",
    )?;
    Ok(id.to_owned())
}
fn metric(value: &Value, field: &str) -> Result<u64> {
    value["summary"][field]
        .as_u64()
        .ok_or(ReleaseError::Invalid("recorded metric is missing"))
}
fn millis(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

struct Recorder {
    start: Instant,
    frames: Vec<Value>,
    commands: Vec<Value>,
    bytes: usize,
}
impl Recorder {
    fn note(&mut self, text: &str) -> Result<()> {
        let text = format!("runner> {text}");
        std::io::stdout().write_all(text.as_bytes())?;
        self.frames.push(
            json!({"elapsed_ms":millis(self.start),"stream":"runner","command":null,"text":text}),
        );
        Ok(())
    }
    fn run(
        &mut self,
        binary: &Path,
        cwd: &Path,
        runtime: &RuntimeOverrides,
        args: &[&str],
        expected: i32,
    ) -> Result<Value> {
        self.note(&format!("attow {}\n", args.join(" ")))?;
        let index = self.commands.len();
        let mut command = Command::new(binary);
        command.args(args).current_dir(cwd);
        runtime.apply(&mut command);
        let output = capture(
            &mut command,
            self.start,
            index,
            Duration::from_secs(300),
            COMMAND_BYTES,
        )?;
        self.bytes = self
            .bytes
            .checked_add(output.stdout.len() + output.stderr.len())
            .ok_or(ReleaseError::Invalid("recording byte overflow"))?;
        require(self.bytes <= RECORDING_BYTES, "recording output limit")?;
        let envelope: Value = serde_json::from_str(&output.stdout)?;
        require(
            output.code == expected && envelope["schema"] == 1 && envelope["command"] == args[0],
            "unexpected recorded command result",
        )?;
        let data = envelope["data"].clone();
        self.commands.push(json!({"index":index,"argv":args,"started_ms":output.started_ms,"ended_ms":output.ended_ms,"exit_code":output.code,"stdout":output.stdout,"stderr":output.stderr,"envelope":envelope}));
        self.frames.extend(output.frames);
        Ok(data)
    }
}

fn cast(events: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(
        &json!({"version":2,"width":110,"height":36,"timestamp":events["started_unix_ms"].as_u64().ok_or(ReleaseError::Invalid("recording start"))?/1000,"title":"Attow Network owned workflow"}),
    )?;
    bytes.push(b'\n');
    for frame in events["frames"]
        .as_array()
        .ok_or(ReleaseError::Invalid("recording frames"))?
    {
        let ms = frame["elapsed_ms"]
            .as_u64()
            .ok_or(ReleaseError::Invalid("frame time"))?;
        let text = frame["text"]
            .as_str()
            .ok_or(ReleaseError::Invalid("frame text"))?;
        serde_json::to_writer(&mut bytes, &json!([ms as f64 / 1000.0, "o", text]))?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}
fn viewer(events: &Value) -> Result<String> {
    let data = serde_json::to_string(events)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Attow Network — actual owned workflow recording</title><style>body{{background:#10151c;color:#e5eef8;font:16px system-ui;margin:2rem auto;max-width:1100px;padding:0 1rem}}button{{padding:.6rem 1rem;margin:.3rem;background:#d8eeff;border:0;border-radius:.3rem}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;background:#090d12;padding:1rem;min-height:25rem;font:13px ui-monospace,monospace}}p{{line-height:1.5}}#clock{{font-variant-numeric:tabular-nums}}</style><h1>Attow Network</h1><p>Actual terminal output from owned WASM, SQL, browser and private peer fixtures. Provider: simulated. The recording preserves observed elapsed times; configured worker limits and observed peaks are distinct. No public providers are contacted.</p><button id="play">Play at recorded speed</button><button id="pause">Pause</button><button id="restart">Restart</button><button id="all">Show complete transcript</button><span id="clock">0.000 s</span><pre id="terminal" aria-live="off"></pre><script id="recorded-data" type="application/json">{data}</script><script>'use strict';const recording=JSON.parse(document.getElementById('recorded-data').textContent),frames=recording.frames,terminal=document.getElementById('terminal'),clock=document.getElementById('clock');let position=0,elapsed=0,origin=0,playing=false;function show(ms){{while(position<frames.length&&frames[position].elapsed_ms<=ms){{terminal.append(document.createTextNode(frames[position].text));position++}}clock.textContent=(ms/1000).toFixed(3)+' s'}}function tick(now){{if(!playing)return;elapsed=now-origin;show(elapsed);if(position<frames.length)requestAnimationFrame(tick);else playing=false}}document.getElementById('play').onclick=()=>{{if(playing)return;playing=true;origin=performance.now()-elapsed;requestAnimationFrame(tick)}};document.getElementById('pause').onclick=()=>{{playing=false}};document.getElementById('restart').onclick=()=>{{playing=false;position=0;elapsed=0;terminal.textContent='';show(0)}};document.getElementById('all').onclick=()=>{{playing=false;elapsed=frames.length?frames[frames.length-1].elapsed_ms:0;show(elapsed)}};show(0);</script></html>"#
    ))
}
struct Captured {
    code: i32,
    stdout: String,
    stderr: String,
    frames: Vec<Value>,
    started_ms: u64,
    ended_ms: u64,
}
enum Pipe {
    Data(usize, Vec<u8>),
    End(usize),
    Error,
}
fn capture(
    command: &mut Command,
    start: Instant,
    index: usize,
    timeout: Duration,
    cap: usize,
) -> Result<Captured> {
    let started_ms = millis(start);
    let deadline = Instant::now() + timeout;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or(ReleaseError::Invalid("stdout pipe"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(ReleaseError::Invalid("stderr pipe"))?;
    let (tx, rx) = mpsc::sync_channel(8);
    let reader = |mut pipe: Box<dyn Read + Send>, stream: usize, tx: mpsc::SyncSender<Pipe>| {
        std::thread::spawn(move || {
            let mut buf = [0; 4096];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Pipe::End(stream));
                        break;
                    }
                    Ok(n) => {
                        if tx.send(Pipe::Data(stream, buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        let _ = tx.send(Pipe::Error);
                        break;
                    }
                }
            }
        })
    };
    let out_thread = reader(Box::new(stdout), 0, tx.clone());
    let err_thread = reader(Box::new(stderr), 1, tx);
    let mut ended = [false; 2];
    let mut raw = [String::new(), String::new()];
    let mut pending = [Vec::<u8>::new(), Vec::<u8>::new()];
    let mut frames = Vec::new();
    let mut bytes = 0usize;
    let result = (|| -> Result<i32> {
        loop {
            if Instant::now() >= deadline {
                return Err(ReleaseError::Tool(
                    "recorded command exceeded its deadline".into(),
                ));
            }
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(Pipe::Data(stream, data)) => {
                    bytes = bytes
                        .checked_add(data.len())
                        .ok_or(ReleaseError::Invalid("capture overflow"))?;
                    require(bytes <= cap, "recorded command output exceeds limit")?;
                    pending[stream].extend(data);
                    let valid = match std::str::from_utf8(&pending[stream]) {
                        Ok(s) => s.len(),
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        Err(_) => return Err(ReleaseError::Invalid("non-UTF8 recorded output")),
                    };
                    if valid > 0 {
                        let text = std::str::from_utf8(&pending[stream][..valid])
                            .map_err(|_| ReleaseError::Invalid("UTF8 output"))?
                            .to_owned();
                        pending[stream].drain(..valid);
                        if stream == 0 {
                            std::io::stdout().write_all(text.as_bytes())?;
                        } else {
                            std::io::stderr().write_all(text.as_bytes())?;
                        }
                        raw[stream].push_str(&text);
                        frames.push(json!({"elapsed_ms":millis(start),"stream":if stream==0{"stdout"}else{"stderr"},"command":index,"text":text}));
                    }
                }
                Ok(Pipe::End(stream)) => {
                    require(pending[stream].is_empty(), "truncated UTF8 recorded output")?;
                    ended[stream] = true;
                }
                Ok(Pipe::Error) => {
                    return Err(ReleaseError::Tool("recorded output read failed".into()));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    require(ended == [true, true], "recorded output disconnected")?
                }
            }
            if ended == [true, true]
                && let Some(status) = child.try_wait()?
            {
                return status.code().ok_or(ReleaseError::Invalid(
                    "recorded command terminated without exit code",
                ));
            }
        }
    })();
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    out_thread
        .join()
        .map_err(|_| ReleaseError::Invalid("stdout reader panicked"))?;
    err_thread
        .join()
        .map_err(|_| ReleaseError::Invalid("stderr reader panicked"))?;
    let [stdout, stderr] = raw;
    Ok(Captured {
        code,
        stdout,
        stderr,
        frames,
        started_ms,
        ended_ms: millis(start),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_child() {
        use std::io::Write;
        match std::env::var("ATTOW_RECORDER_TEST").as_deref() {
            Ok("streams") => {
                println!("actual child: <script>λ</script>");
                std::io::stdout().flush().expect("flush");
                eprintln!("actual child stderr");
            }
            Ok("wait") => std::thread::sleep(std::time::Duration::from_secs(5)),
            Ok("large") => {
                print!("{}", "x".repeat(65_536));
                std::io::stdout().flush().expect("flush");
            }
            _ => {}
        }
    }
    fn child(mode: &str) -> std::process::Command {
        let mut c = std::process::Command::new(std::env::current_exe().expect("test executable"));
        c.args([
            "--exact",
            "release::demo_record::tests::capture_child",
            "--nocapture",
        ])
        .env("ATTOW_RECORDER_TEST", mode);
        c
    }
    #[test]
    fn captures_real_subprocess_streams_and_reconstructs_them_exactly() -> Result<()> {
        let output = capture(
            &mut child("streams"),
            std::time::Instant::now(),
            3,
            std::time::Duration::from_secs(10),
            256 * 1024,
        )?;
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("actual child: <script>λ</script>"));
        assert!(output.stderr.contains("actual child stderr"));
        for (name, text) in [("stdout", output.stdout), ("stderr", output.stderr)] {
            let assembled: String = output
                .frames
                .iter()
                .filter(|f| f["stream"] == name)
                .map(|f| f["text"].as_str().expect("text"))
                .collect();
            assert_eq!(text, assembled);
        }
        assert!(output.frames.iter().all(|f| f["command"] == 3));
        assert!(output.ended_ms >= output.started_ms);
        Ok(())
    }
    #[test]
    fn subprocess_capture_has_a_total_deadline_and_output_bound() {
        assert!(
            capture(
                &mut child("wait"),
                std::time::Instant::now(),
                0,
                std::time::Duration::from_millis(100),
                1024
            )
            .is_err()
        );
        assert!(
            capture(
                &mut child("large"),
                std::time::Instant::now(),
                0,
                std::time::Duration::from_secs(10),
                1024
            )
            .is_err()
        );
    }
    #[test]
    fn terminal_frames_preserve_the_exact_observed_stream_and_timing() -> Result<()> {
        let events = json!({"started_unix_ms":1234000,"frames":[{"elapsed_ms":0,"stream":"runner","command":null,"text":"runner> objective\n"},{"elapsed_ms":175,"stream":"stdout","command":0,"text":"{\"schema\":1}\n"},{"elapsed_ms":205,"stream":"stderr","command":0,"text":"note\r\n"}]});
        let bytes = cast(&events)?;
        let lines: Vec<Value> = std::str::from_utf8(&bytes)
            .expect("UTF8 cast")
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0]["version"], 2);
        assert_eq!(lines[1], json!([0.0, "o", "runner> objective\n"]));
        assert_eq!(lines[2], json!([0.175, "o", "{\"schema\":1}\n"]));
        assert_eq!(lines[3], json!([0.205, "o", "note\r\n"]));
        Ok(())
    }
    #[test]
    fn local_viewer_keeps_hostile_terminal_data_inert_and_embedded() -> Result<()> {
        let events = json!({"frames":[{"elapsed_ms":0,"text":"</script><img src=x onerror=alert(1)>&\u{2028}"}]});
        let html = viewer(&events)?;
        assert!(html.contains("textContent"));
        assert!(html.contains("\\u003c/script\\u003e"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("innerHTML"));
        assert!(!html.contains("fetch("));
        assert!(!html.contains("https://"));
        Ok(())
    }
}
