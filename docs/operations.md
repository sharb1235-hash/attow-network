# Local operation and recovery

Each initialized workspace keeps its authority, immutable store, run catalog and
owned simulator data under `.attow`. The owned demo fence is a separate directory
with a process-exclusive OS lock and FULL-synchronous checkpoint anchor. It is
excluded from portable exports. It demonstrates same-host exclusive resumption;
it is not a cross-host fence or a credential service.

Only workspace initialization enrolls a fresh local authority. Existing runs open
the saved fence and require the exact externally anchored head. Copying an
authority database alone never resumes a dispatcher. Copying the fence itself is
unsupported and invalidates this demo's trust assumption. Use the
[external fencing contract](authority-handoff.md) for deployment integration.

Before promotion, the CLI saves a Prepared record with fixed request, decision,
generation, descriptor, payload and logical operation IDs. It saves the returned
intent before dispatch. Resume retries the same promotion request; it never
generates a replacement operation to conceal an uncertain outcome.

`Authorized` can dispatch through the gateway. A durable `Dispatching` left by a
crash becomes `Unknown`; Unknown is reconciled using the original provider
contract. It remains visibly unresolved when evidence cannot determine the
outcome. The simulator's private evidence key stays in its database and never
enters exports or logs.

The checkout fixture deliberately loses a response after the simulator commits.
`--pause-after-unknown` exits 3 after persisting that state. Resume with
`attow run checkout --resume RUN_ID --json`; preserve the complete native workspace.
Receipt-dependent work runs again with the confirmed receipt before its response
branch can promote. This is separate from replay, which never executes workers.

`attow peer serve --bind 127.0.0.1:0` creates a diagnostic loopback listener with
a default 30-second lifetime; `--duration-ms` accepts 1–300000. Its empty peer
allowlist denies all peers and proposals. It reports the actual address after
closing cleanly. Applications must configure peer allowlists and proposal
callbacks through the library APIs. The integrated owned demo composes two
authenticated private endpoints and an explicitly held authority fence itself.

Use `verify` on the native workspace for authority replay and portable graph/trace
checks. Use `export` for portable evidence. Import never activates authority and
offers no force flag. A hash-valid portable receipt demonstrates retained evidence;
it does not establish public provider truth or authenticate an external maintainer.

Ledger recovery semantics are documented in [commit domains](commit-domains.md) and
[authority handoff](authority-handoff.md). Do not delete a fence, change a writer ID,
or copy a database into place to work around a fenced error. The demo provides no
automatic recovery for a ledger commit whose external anchor update failed.
