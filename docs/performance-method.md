# Performance measurement method

`cargo run -p xtask --locked -- bench-report --output target/bench-report.json`
invokes the actual release-profile worldline and scheduler harnesses and retains
their raw JSON samples. It rejects failed commands and smoke-mode-only output.
CLI demo event counts and browser recordings are separate functional evidence;
they do not imply a performance improvement or a benchmark result.

The `worldline` benchmark measures Stage 3 logical forks and durable state
admission. It does not create a worker, process, browser, SQL execution instance,
or portable backend checkpoint. Worker materialization is not measured.

Run from the repository root with the pinned toolchain:

```sh
cargo +1.95.0 bench --bench worldline --locked -- --entries 1000 --forks 1000 --samples 5
```

The harness emits newline-delimited JSON on standard output. Cargo build messages
are separate process output. Preserve the raw JSON, exact command, repository
revision, and whether the checkout contained uncommitted changes alongside any
derived comparison. Environment records include OS, architecture, visible logical
CPU count, compiler version when available, debug-assertion state, and the current
Git commit and working-tree dirty state when available. The Windows processor identifier is reported as an
environment value, not an independently verified hardware measurement. Missing
metadata is `null`.

Each sample reports three separate durations:

- `logical_fork`: creates and retains the requested number of branch descriptors
  from one already-loaded root. Vector allocation, root construction, and branch
  destruction occur outside the timed section. Nonce construction, identity
  hashing, shared-root reference counts, and descriptor construction are included.
- `durable_root_admission`: persists and pins the root in one FULL-synchronous
  SQLite transaction. The first sample writes a new object; later samples reuse
  that object and add distinct owner pins. Records identify this difference.
- `durable_branch_admission`: admits one frozen branch through its own SQLite
  transaction, with the base already persisted. Freezing is outside the timed
  section. Post-write verification and reopen checks are also outside timing.

Durable measurements use a new temporary database for each benchmark invocation.
They include operating-system and device behavior and are not comparable to an
in-memory fork measurement. No allocation count, resident-worker count, peak RSS,
or physical snapshot size is inferred from elapsed time.

Bounds are 1–4,096 state entries, 1–100,000 forks per sample, and 1–30 samples.
Defaults are 1,000 entries, 1,000 forks, and 5 samples. Cargo's `--test` marker runs
a 32-entry, 16-fork, one-sample smoke path for the all-targets test gate. Test-mode
timings are not release-profile benchmark evidence. Errors produce a failing exit
status and must not be interpreted as successful samples.

The root is constructed before timing. Its initial construction and durable load
may scale with the full state size; cheap logical fork does not imply cheap durable
admission or execution. There is no latency threshold assertion in CI. Compare
multiple repeated runs on stated hardware, retain outliers, and describe warmup,
build profile, sample size, operating-system load, and storage conditions before
publishing performance conclusions. Later scheduler benchmarks must separately
report logical branches, admitted work, and resident workers.

No cross-platform or production performance claim is established by this harness.
Only explicitly executed runs recorded in the progress evidence are measurements.

## Recorded local run

The following is one executed three-sample run on the current Windows checkout,
using the optimized Cargo bench profile with debug information. It verifies the
harness and provides raw observations; three samples do not establish a general
latency guarantee. The checkout had uncommitted files and no Git commit yet.

```sh
cargo +1.95.0 bench --bench worldline --locked -- --entries 1000 --forks 1000 --samples 3
```

Standard output from this successful run:

```jsonl
{"record":"environment","schema":1,"project":"Attow Network","package_version":"0.1.0","os":"windows","arch":"x86_64","logical_cpus":8,"debug_assertions":false,"rustc":"rustc 1.95.0 (59807616e 2026-04-14)\nbinary: rustc\ncommit-hash: 59807616e1fa2540724bfbac14d7976d7e4a3860\ncommit-date: 2026-04-14\nhost: x86_64-pc-windows-msvc\nrelease: 1.95.0\nLLVM version: 22.1.2","git_commit":null,"git_dirty":true,"cpu_identifier_environment":"Intel64 Family 6 Model 165 Stepping 2, GenuineIntel","entries":1000,"forks_per_sample":1000,"samples":3,"materialization_measured":false}
{"record":"sample","metric":"logical_fork","sample":0,"operations":1000,"elapsed_ns":452700,"held_branches":1000,"destruction_included":false}
{"record":"sample","metric":"durable_root_admission","sample":0,"operations":1,"elapsed_ns":8607200,"existing_object_reuse":false}
{"record":"sample","metric":"durable_branch_admission","sample":0,"operations":1,"elapsed_ns":3195700,"base_already_persisted":true}
{"record":"sample","metric":"logical_fork","sample":1,"operations":1000,"elapsed_ns":413600,"held_branches":1000,"destruction_included":false}
{"record":"sample","metric":"durable_root_admission","sample":1,"operations":1,"elapsed_ns":4235800,"existing_object_reuse":true}
{"record":"sample","metric":"durable_branch_admission","sample":1,"operations":1,"elapsed_ns":3112700,"base_already_persisted":true}
{"record":"sample","metric":"logical_fork","sample":2,"operations":1000,"elapsed_ns":322300,"held_branches":1000,"destruction_included":false}
{"record":"sample","metric":"durable_root_admission","sample":2,"operations":1,"elapsed_ns":3432300,"existing_object_reuse":true}
{"record":"sample","metric":"durable_branch_admission","sample":2,"operations":1,"elapsed_ns":3878500,"base_already_persisted":true}
```

## Scheduler measurement method

The separate `scheduler` harness measures Stage 6 graph submission through a
completed run, including admission checks, queueing, worker execution, result
retention, and semantic trace construction. It creates a fresh scheduler and
cache for each sample. Fixture construction, code compilation, input creation,
runtime construction, and cleanup are outside the timed section. The actual
WASM component is compiled before timing but has no prior evaluation in that
sample. These are warm compilation measurements, not cold process startup.

```sh
cargo +1.95.0 bench --bench scheduler --features scheduler --locked -- --nodes 8 --samples 3
```

Two backends are reported separately. `mock_cpu_blake3_memory_repository` runs
500 deterministic BLAKE3 rounds and retains canonical result objects in memory;
it measures scheduler mechanics with a synthetic workload. The resource-class
tag is a routing key and does not make this mock a WASM measurement.
`actual_wasmtime_component_sqlite_full` executes the original arithmetic
component through the admitted sandbox backend and retains its complete result
graph using FULL-synchronous SQLite. Each real sample has a fresh temporary
database. The CPU loop and the arithmetic component are different workloads;
their latencies are not a comparison of backend speed.

Each graph has two nodes per logical branch. In `common_prefix`, the first node
of every branch has identical immutable inputs, followed by a distinct suffix.
`no_sharing` uses the same graph with reuse disabled. `high_divergence` gives
every node distinct inputs. The harness checks actual backend call counts:
half the node count plus one for common prefixes, and the full node count for
the other two scenarios. Branch identities and attempt identities remain
distinct. Worker counts describe executed evaluations, not resident agents.
The resident limit is two; a sample reports the scheduler's observed peak.

The `replay` sample consumes the common-prefix trace and recorded immutable
objects in memory. Gathering the records from SQLite is outside this timing.
Replay verifies the graph and objects, checks exact output equality, and the
harness verifies that the backend call counter does not advance. It is not a
measurement of database restore, transport, or external-provider replay.

The JSONL stream contains raw sample durations and separate nearest-rank
p50/p95/p99 summaries. Metadata includes OS, architecture, logical CPU count,
the Windows processor environment string when available, exact compiler,
repository revision and dirty state, executable/lock/harness/fixture BLAKE3
hashes, and each backend's canonical code/profile identifiers. The executable
hash identifies the code actually measured even when the checkout is dirty.
Small sample counts are useful for checking the harness; their percentiles do
not establish latency guarantees. Retain outliers and compare repeated runs
under stated machine load and storage conditions before making performance
claims. The benchmark does not call a network, model, browser, or payment
provider.

Bounds are an even 4–64 nodes and 1–20 samples. Defaults are eight nodes and three
samples; debug builds or an explicit `--test` argument select four nodes and one sample. Test-mode
output is smoke evidence, not release benchmark evidence. Invalid arguments,
incomplete runs, unexpected evaluation counts, and replay mismatches fail the
process. No performance threshold is asserted in CI.

## Recorded scheduler run

This command completed successfully on 2026-10-10 using the final Stage 6 source
on the local Windows x86-64 workstation. The optimized bench profile retained
debug information. The checkout was dirty, based on commit
`017626e96183af6f4ce4c9d5a3bbf2d0cad6f960`; the raw executable hash identifies
the measured build. The machine had eight visible logical CPUs and reported
`Intel64 Family 6 Model 165 Stepping 2, GenuineIntel` in its processor environment
value. This was a development workstation run, not an isolated performance lab.

```sh
cargo +1.95.0 bench -j 1 --bench scheduler --features scheduler --locked -- --nodes 8 --samples 3
```

[Raw JSONL samples and metadata](research/stage6-scheduler-2026-10-10.jsonl)
contain one environment record, 24 sample records, and eight summaries. Their
SHA-256 is `7f0252f8e8e6b23cab3e77f2beb17371ec459544dbb721e61b9db1b82200a64a`.
All records parsed successfully; the sample counts, worker-call counts, resident
limit, and percentile calculations were independently checked after execution.
Each row below has three samples of an eight-node, four-branch graph.

| Backend | Scenario | Worker calls per sample | p50 ms | p95 ms | p99 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| Mock CPU / memory | Common prefix | 5 | 0.4017 | 0.8311 | 0.8311 |
| Mock CPU / memory | No sharing | 8 | 0.4823 | 0.5233 | 0.5233 |
| Mock CPU / memory | High divergence | 8 | 0.3771 | 0.4660 | 0.4660 |
| Mock CPU / memory | Replay | 0 | 0.1119 | 0.2571 | 0.2571 |
| Actual WASM / SQLite | Common prefix | 5 | 43.6497 | 46.0450 | 46.0450 |
| Actual WASM / SQLite | No sharing | 8 | 37.4871 | 43.7492 | 43.7492 |
| Actual WASM / SQLite | High divergence | 8 | 51.9996 | 62.1817 | 62.1817 |
| Actual WASM / SQLite | Replay | 0 | 0.1488 | 0.1813 | 0.1813 |

The common-prefix graph executed five evaluations instead of eight while
preserving all branch outputs, and replay invoked no workers. Its actual WASM
median was slower than the no-sharing median in this small run; fewer evaluations
do not by themselves establish a latency improvement. Cache retention adds work,
and three samples on an active workstation do not separate individual cost
sources. With three samples, nearest-rank p95 and p99 both equal the maximum.
No throughput, cross-platform, tail-latency, or production-scale claim follows
from these observations.
