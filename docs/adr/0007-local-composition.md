# ADR 0007: Share local composition between CLI and facade

Status: accepted for Stage 9.

The earlier stages exposed SQLite implementations of state, sandbox and scheduler
ports through the facade. The installed CLI must compose those implementations
without depending on the facade or copying its algorithms.

Introduce `attow-local` under `crates/chronos-local` as an explicit prerequisite.
Move the existing persistence wrappers, result repository, confirmed response
guard and peer-authority callback into that package. The facade reexports their
original public paths. Runtime ports remain in their owning state/scheduler/
transport crates; transport still has no ledger dependency.

`attow-local` has empty default features, mandatory local codec/state/store/ledger
dependencies, and optional sandbox/scheduler/peer composition. The CLI uses those
features directly. The optional browser adapter is activated separately.

The package also provides an explicitly owned same-host demo fence with an OS
exclusive file lock and an external durable head anchor. This is a fixture for
restart demonstrations, not the production distributed fencing implementation.
Its directory never appears in portable exports. Only fresh workspace
initialization enrolls it; resume must open the saved exact anchor.

Alternatives rejected: a CLI-to-facade dependency would reverse the architecture;
copying persistence or promotion code would let their validation diverge. This
additional small composition package preserves one implementation and existing
facade API paths while enabling actual source installation.
