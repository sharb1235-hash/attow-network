use crate::{CliError, demo::Scenario, workspace::validate_id};
use std::{ffi::OsString, path::PathBuf};
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Init(PathBuf),
    Doctor,
    Run {
        scenario: Scenario,
        workers: usize,
        resume: Option<String>,
        browser: bool,
        pause_after_unknown: bool,
    },
    BranchList,
    Compare(String, String),
    Promote {
        branch: String,
        decision: String,
        generation: u64,
        request_id: String,
    },
    Replay(String),
    Verify(String),
    Export {
        run: String,
        output: PathBuf,
    },
    Import(PathBuf),
    PeerServe {
        bind: String,
        duration_ms: u64,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub command: Command,
    pub json: bool,
    pub root: Option<PathBuf>,
}
impl Args {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let mut words = Vec::new();
        for arg in args {
            if words.len() >= 32 || arg.len() > 4096 {
                return Err(bad());
            }
            words.push(
                arg.into_string()
                    .map_err(|_| CliError::input("arguments must be Unicode"))?,
            );
        }
        let json = flag(&mut words, "--json")?;
        let root = option(&mut words, "--root")?.map(PathBuf::from);
        let command = match words.first().map(String::as_str) {
            None | Some("--help" | "-h") if words.len() <= 1 => Command::Help,
            Some("--version" | "-V") if words.len() == 1 => Command::Version,
            Some("init") if words.len() == 2 && !words[1].starts_with('-') => {
                Command::Init(words[1].clone().into())
            }
            Some("doctor") if words.len() == 1 => Command::Doctor,
            Some("run") => {
                let workers = option(&mut words, "--workers")?
                    .map(|n| n.parse::<usize>().map_err(|_| bad()))
                    .transpose()?
                    .unwrap_or(2);
                if !(1..=8).contains(&workers) {
                    return Err(CliError::input("workers must be between 1 and 8"));
                }
                let resume = option(&mut words, "--resume")?;
                if let Some(id) = &resume {
                    validate_id(id)?;
                }
                let browser = flag(&mut words, "--browser")?;
                let pause_after_unknown = flag(&mut words, "--pause-after-unknown")?;
                if words.len() != 2 {
                    return Err(bad());
                }
                let scenario = match words[1].as_str() {
                    "checkout" => Scenario::Checkout,
                    "data-change" => Scenario::DataChange,
                    _ => return Err(CliError::input("scenario must be checkout or data-change")),
                };
                if pause_after_unknown && scenario != Scenario::Checkout {
                    return Err(bad());
                }
                Command::Run {
                    scenario,
                    workers,
                    resume,
                    browser,
                    pause_after_unknown,
                }
            }
            Some("branch") if words.as_slice() == ["branch", "list"] => Command::BranchList,
            Some("compare") if words.len() == 3 => {
                validate_id(&words[1])?;
                validate_id(&words[2])?;
                Command::Compare(words[1].clone(), words[2].clone())
            }
            Some("promote") => {
                let decision = required(&mut words, "--decision")?;
                validate_id(&decision)?;
                let generation = required(&mut words, "--generation")?
                    .parse()
                    .map_err(|_| bad())?;
                let request_id = required(&mut words, "--request-id")?;
                validate_id(&request_id)?;
                if words.len() != 2 {
                    return Err(bad());
                }
                validate_id(&words[1])?;
                Command::Promote {
                    branch: words[1].clone(),
                    decision,
                    generation,
                    request_id,
                }
            }
            Some("replay") => {
                if !flag(&mut words, "--offline")? || words.len() != 2 {
                    return Err(CliError::input("replay requires RUN --offline"));
                }
                validate_id(&words[1])?;
                Command::Replay(words[1].clone())
            }
            Some("verify") if words.len() == 2 => {
                validate_id(&words[1])?;
                Command::Verify(words[1].clone())
            }
            Some("export") => {
                let output = required(&mut words, "--output")?.into();
                if words.len() != 2 {
                    return Err(bad());
                }
                validate_id(&words[1])?;
                Command::Export {
                    run: words[1].clone(),
                    output,
                }
            }
            Some("import") => {
                if !flag(&mut words, "--data-only")?
                    || words.len() != 2
                    || words[1].starts_with('-')
                {
                    return Err(CliError::input("import requires PATH --data-only"));
                }
                Command::Import(words[1].clone().into())
            }
            Some("peer") => {
                let bind = required(&mut words, "--bind")?;
                let duration_ms = option(&mut words, "--duration-ms")?
                    .map(|n| n.parse::<u64>().map_err(|_| bad()))
                    .transpose()?
                    .unwrap_or(30_000);
                if words.as_slice() != ["peer", "serve"] || !(1..=300_000).contains(&duration_ms) {
                    return Err(bad());
                }
                Command::PeerServe { bind, duration_ms }
            }
            _ => return Err(bad()),
        };
        Ok(Self {
            command,
            json,
            root,
        })
    }
}
fn bad() -> CliError {
    CliError::input("invalid or duplicate arguments; see attow --help")
}
fn option(words: &mut Vec<String>, name: &str) -> Result<Option<String>, CliError> {
    let positions: Vec<_> = words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| (w == name).then_some(i))
        .collect();
    match positions.as_slice() {
        [] => Ok(None),
        [i] => {
            if *i + 1 >= words.len() || words[*i + 1].starts_with('-') {
                return Err(bad());
            }
            let value = words.remove(*i + 1);
            words.remove(*i);
            Ok(Some(value))
        }
        _ => Err(bad()),
    }
}
fn required(words: &mut Vec<String>, name: &str) -> Result<String, CliError> {
    option(words, name)?.ok_or_else(bad)
}
fn flag(words: &mut Vec<String>, name: &str) -> Result<bool, CliError> {
    let positions: Vec<_> = words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| (w == name).then_some(i))
        .collect();
    match positions.as_slice() {
        [] => Ok(false),
        [i] => {
            words.remove(*i);
            Ok(true)
        }
        _ => Err(bad()),
    }
}
