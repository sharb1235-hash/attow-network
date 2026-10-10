# Browser fidelity and supported snapshots

The optional browser adapter runs the repository's original loopback fixture in a
real Chromium process. It accepts `OwnedEnvironment` mode with credentials disabled.
It rejects `Live`, arbitrary websites, arbitrary scripts, hidden JavaScript stack
restoration and a `Pure` worker label. Checkout previews explicitly use `Model`
fidelity; they contain no purchase confirmation.

The pinned runtime is Node 24.21.0, Playwright 1.64.0 and Chromium
156.0.8078.4, Playwright revision 1248. The Rust bridge compares the sidecar and
fixture files to the bytes included at compilation. The sidecar checks the Node,
Playwright, browser revision and running browser versions before reporting ready.
Code and profile identities include the original source hashes, runtime versions,
platform and declared limits. Archive, registry and license evidence is retained in
[Stage 7 provenance](research/stage7-browser-provenance.json).

## Storage contract

Restoration creates a fresh browser context, loads the owned fixture and imports
only the following versioned storage representation. Each context is separate;
siblings can restore the same snapshot and subsequently diverge.

| Storage | Supported representation | Excluded behavior |
|---|---|---|
| Cookies | At most 128 fixture session cookies: `127.0.0.1`, path `/`, `SameSite=Lax`, non-secure and non-HttpOnly | Arbitrary domains, persistent/authentication cookies and remote account isolation |
| localStorage | At most 128 unique string key/value pairs for the owned origin | Other origins or browser profile state |
| sessionStorage | At most 128 unique string pairs, explicitly exported and imported into the new page | Implicit preservation of a prior tab or process |
| IndexedDB | Fixed `attow-fixture` database, version 1, `values` store, bounded string pairs | Arbitrary schemas, database versions, object graphs or live transactions |
| JavaScript execution | No stack or heap checkpoint | Closures, pending promises, timers, DOM execution state and active network requests |

Keys are nonempty and limited to 128; values are limited to 4,096. The Rust entry
point applies byte limits and the sidecar also validates its string representation.
The entire serialized snapshot is capped at 256 KiB. A storage update that exceeds
the snapshot bound restores the previous snapshot and reports an error.

Snapshots are content-addressed objects bound to the tenant, security scope and
browser profile. Restore checks all three and rejects a different owner. The
payload is **not encrypted at rest**; the supported environment has no live
credentials. The host must protect its database and backups. Playwright separately
warns that saved authenticated state may contain account credentials, and documents
explicit handling for session storage. This adapter admits only its narrower
fixture representation. [Playwright authentication documentation](https://playwright.dev/docs/auth)

## Requests, observations and limits

The public observation allowlist is `snapshot` and `checkout_preview`, both without
arguments. Storage and negative network probes are fixture control methods. The
sidecar protocol uses a four-byte big-endian length, version 1, sequential request
IDs and exact message fields. Frames cap at 512 KiB. Serialized snapshots cap at
256 KiB; the same observation limit applies to the entire canonical observation
object, including metadata and references. Each bridge exchange has a ten-second
deadline. A worker step uses two exchanges, restore and observe, so it can take up
to twenty seconds before bounded teardown. Teardown can add a ten-second close
exchange and a two-second process-exit wait before a forced kill. A failed exchange
retires its process rather than reusing a possibly desynchronized stream.

The host admits at most four resident browser bridges, bounds the queue, and holds
one bridge lease across restore and observation. The worker verifies tenant,
scope, policy, code, profile, fidelity, effect class and resource budget. Browser
workers are `FreshObservation`; results retain their capture time and provenance
instead of being reused as pure evaluations. The declared memory and fuel fields
bound adapter work and buffers; they do **not** enforce Chromium RSS or CPU quotas.

The fixture serves fixed local assets with a restrictive CSP. Request routing
allows only the owned origin and fixed GET paths, WebSocket routing closes
connections, and service workers are blocked. A second owned loopback server
counts attempted forbidden requests. These controls exercise the declared fixture
scope; they are not an OS firewall or proof that a hostile browser process cannot
egress. Playwright documents that service workers can bypass normal route handling,
which is why this context blocks them. [Playwright network documentation](https://playwright.dev/docs/network)

## Running and evidence

Use the exact Node executable and run `npm ci` in
`demos/workflow-studio/browser`. Set `PLAYWRIGHT_BROWSERS_PATH` to that directory's
absolute `.browsers` path before running the locally installed
`node_modules/playwright/cli.js install chromium`. Set `ATTOW_NODE` to the absolute
Node executable path when running Rust conformance tests. The bridge itself uses
the sidecar's `.browsers` directory. Playwright associates each release with its
own browser binaries; the checked-in revision is also verified at startup.
[Playwright browser installation](https://playwright.dev/docs/browsers)

The dedicated Windows CI job installs the verified Node archive, performs locked
npm installation, installs local Chromium, and runs provider, browser and receipt
integration gates. It is configuration, not evidence of a completed remote run.
Local commands and observed results are recorded in [progress](progress.md).
No hostile-site or live-credential deployment is supported on any platform in
this stage; see [adapter security](adapter-security.md).
