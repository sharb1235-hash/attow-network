# Original owned browser fixture

The executable fixture lives in `demos/workflow-studio/app/index.html` and
`app.js`; the original adapter process is `demos/workflow-studio/browser/sidecar.mjs`.
The HTML and JavaScript were authored for Attow Network under MIT OR Apache-2.0.
No external page, screenshot, account, cookie export or third-party application
source was copied. Their byte hashes are recorded in `fixtures/manifest.toml`.

The page describes one predicted fixture seat and has no purchase endpoint.
Conformance tests generate temporary cookie, localStorage, sessionStorage and
IndexedDB values. A second owned loopback server counts prohibited fetch and
WebSocket requests; the sidecar's owned service-worker route counts registration
attempts. These are local negative probes, not public-site traffic.

Playwright and Chromium are separate third-party runtimes, not original fixture
code. Their exact package/browser provenance and retained notices are recorded in
`docs/research/stage7-browser-provenance.json` and `docs/notices/stage7`.
See `docs/browser-fidelity.md` for the admitted storage representation and limits.
