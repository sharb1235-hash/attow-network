//! Raw, bounded samples for logical forks and durable admission. No worker runs.

use std::{
    collections::BTreeMap, error::Error, hint::black_box, io, process::Command, time::Instant,
};

use attow_network::{
    state::{DomainId, Mutation, StateRoot, fork},
    store::Store,
};

#[path = "../tests/support/state_store.rs"]
mod state_store;
use state_store::{StoreAdapter, StoreView};

struct Settings {
    entries: usize,
    forks: usize,
    samples: usize,
}

fn settings() -> Result<Settings, Box<dyn Error>> {
    let mut settings = Settings {
        entries: 1000,
        forks: 1000,
        samples: 5,
    };
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--test" {
            return Ok(Settings {
                entries: 32,
                forks: 16,
                samples: 1,
            });
        }
        // Cargo may forward this marker for a harness=false benchmark.
        if argument == "--bench" {
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "benchmark option needs a value",
                )
            })?
            .parse::<usize>()?;
        match argument.as_str() {
            "--entries" if (1..=attow_network::state::MAX_KEYS).contains(&value) => {
                settings.entries = value
            }
            "--forks" if (1..=100_000).contains(&value) => settings.forks = value,
            "--samples" if (1..=30).contains(&value) => settings.samples = value,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown benchmark option or value outside documented bounds",
                )
                .into());
            }
        }
    }
    Ok(settings)
}

fn json_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control <= '\u{1f}' => {
                escaped.push_str(&format!("\\u{:04x}", u32::from(control)))
            }
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

fn command_text(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}

fn optional_json(value: Option<String>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| json_string(&value))
}

fn main() -> Result<(), Box<dyn Error>> {
    let settings = settings()?;
    let domain = DomainId([11; 32]);
    let mutations = (0..settings.entries)
        .map(|index| (index.to_be_bytes().to_vec(), Mutation::Put(vec![42; 16])))
        .collect::<BTreeMap<_, _>>();
    let root = StateRoot::new(domain).apply(&mutations)?;
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("benchmark.db"))?;
    println!(
        "{{\"record\":\"environment\",\"schema\":1,\"project\":\"Attow Network\",\"package_version\":{},\"os\":{},\"arch\":{},\"logical_cpus\":{},\"debug_assertions\":{},\"rustc\":{},\"git_commit\":{},\"git_dirty\":{},\"cpu_identifier_environment\":{},\"entries\":{},\"forks_per_sample\":{},\"samples\":{},\"materialization_measured\":false}}",
        json_string(env!("CARGO_PKG_VERSION")),
        json_string(std::env::consts::OS),
        json_string(std::env::consts::ARCH),
        std::thread::available_parallelism()
            .map_or_else(|_| "null".to_owned(), |count| count.get().to_string()),
        cfg!(debug_assertions),
        optional_json(command_text(
            "rustc",
            &["+1.95.0", "--version", "--verbose"]
        )),
        optional_json(command_text("git", &["rev-parse", "HEAD"])),
        command_text("git", &["status", "--porcelain"]).map_or_else(
            || "null".to_owned(),
            |status| (!status.is_empty()).to_string()
        ),
        optional_json(std::env::var("PROCESSOR_IDENTIFIER").ok()),
        settings.entries,
        settings.forks,
        settings.samples,
    );

    // Warm only the logical-fork path. Persistence is reported separately below.
    drop(black_box(fork(domain, &root, [1; 32])?));
    for sample in 0..settings.samples {
        let mut branches = Vec::with_capacity(settings.forks);
        let started = Instant::now();
        for index in 0..settings.forks {
            let mut nonce = [0_u8; 32];
            nonce[..8].copy_from_slice(&(index as u64).to_be_bytes());
            nonce[8..16].copy_from_slice(&(sample as u64).to_be_bytes());
            nonce[16] = 2;
            branches.push(black_box(fork(domain, black_box(&root), nonce)?));
        }
        let elapsed = started.elapsed().as_nanos();
        println!(
            "{{\"record\":\"sample\",\"metric\":\"logical_fork\",\"sample\":{sample},\"operations\":{},\"elapsed_ns\":{elapsed},\"held_branches\":{},\"destruction_included\":false}}",
            settings.forks,
            branches.len()
        );
        let branch = branches
            .pop()
            .ok_or_else(|| io::Error::other("fork sample produced no branch"))?;
        drop(branches);

        let started = Instant::now();
        let root_id = root.persist(&mut StoreAdapter(&mut store), &format!("root:{sample}"))?;
        let root_elapsed = started.elapsed().as_nanos();
        let frozen = branch.freeze()?;
        let started = Instant::now();
        let branch_id = frozen.admit(&mut StoreAdapter(&mut store), &format!("branch:{sample}"))?;
        let branch_elapsed = started.elapsed().as_nanos();
        // Verify admitted state outside the measured section.
        let reopened = StateRoot::reopen(&StoreView(&store), root_id, domain)?;
        if reopened.object_id()? != root.object_id()? || !store.contains(branch_id)? {
            return Err(io::Error::other("durable benchmark verification failed").into());
        }
        println!(
            "{{\"record\":\"sample\",\"metric\":\"durable_root_admission\",\"sample\":{sample},\"operations\":1,\"elapsed_ns\":{root_elapsed},\"existing_object_reuse\":{}}}",
            sample != 0
        );
        println!(
            "{{\"record\":\"sample\",\"metric\":\"durable_branch_admission\",\"sample\":{sample},\"operations\":1,\"elapsed_ns\":{branch_elapsed},\"base_already_persisted\":true}}"
        );
    }
    Ok(())
}
