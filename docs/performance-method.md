# Performance measurement method

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
