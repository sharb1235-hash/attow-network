use super::Result;
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) const MAX_CAPTURE: usize = 16 * 1024 * 1024;
pub(super) struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub elapsed_ms: u128,
}
pub(super) fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
pub(super) fn cargo(root: &Path, arguments: &[OsString]) -> Result<Output> {
    let mut all = vec![OsString::from("+1.95.0")];
    all.extend_from_slice(arguments);
    capture(OsStr::new("cargo"), &all, root, Duration::from_secs(3600))
}
pub(super) fn capture(
    program: &OsStr,
    arguments: &[OsString],
    cwd: &Path,
    timeout: Duration,
) -> Result<Output> {
    let file = tempfile::NamedTempFile::new()?;
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file.reopen()?))
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut next_update = Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let kill = child.kill();
            let wait = child.wait();
            kill?;
            wait?;
            return Err("subprocess exceeded its execution deadline".into());
        }
        if started.elapsed() >= next_update {
            eprintln!(
                "Attow Network check is still running ({} seconds).",
                started.elapsed().as_secs()
            );
            next_update += Duration::from_secs(30);
        }
        if file.as_file().metadata()?.len() > MAX_CAPTURE as u64 {
            let kill = child.kill();
            let wait = child.wait();
            kill?;
            wait?;
            return Err("subprocess output exceeded its recording limit".into());
        }
        thread::sleep(Duration::from_millis(100));
    };
    if file.as_file().metadata()?.len() > MAX_CAPTURE as u64 {
        return Err("subprocess output exceeded its recording limit".into());
    }
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout: fs::read(file.path())?,
        elapsed_ms: started.elapsed().as_millis(),
    })
}
pub(super) fn success(output: &Output) -> Result<()> {
    if output.code != 0 {
        return Err(format!("subprocess failed with exit {}", output.code).into());
    }
    Ok(())
}
