# Stage 8 — Authenticated QUIC object exchange and domain authority

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: a local broad vertical slice works. Add local-first federation: peers exchange immutable worlds and send proposals to the designated authority. This stage does not manufacture consensus from QUIC.

Files: crates/chronos-protocol/src/{lib,frame,messages,version,limits}.rs; crates/chronos-transport/src/{lib,endpoint,session,objects,authority}.rs; tests/peer_protocol.rs; fixtures/original/protocol; fuzz_targets/frame.rs and worldline_import.rs; docs/{protocol-v1,peer-trust,authority-handoff}.md; deploy/relay example marked optional.

Pins: iroh=1.3.0 default-features=false features tls-ring; tokio=1.53.2 rt-multi-thread,macros,sync,time,io-util; ed25519-dalek=3.0.0; getrandom=0.4.3; zeroize=1.9.1; existing codec/protocol; ledger is an integration-test dev-dependency only; test tempfile=3.27.0/proptest=1.11.0. Iroh uses noq internally; do not introduce Quinn as if its types were interchangeable. No Raft crate in v0.1.

API:
```rust
pub enum Message { Hello(Hello), OfferObjects(Vec<ObjectId>), WantObjects(Vec<ObjectId>), ObjectChunk(Chunk), BranchProposal(Proposal), PromotionResult(ResultMessage), SignedCheckpoint(Checkpoint), Ack(Ack), ProtocolError(ErrorMessage) }
pub fn encode_frame(msg:&Message) -> Result<Vec<u8>, ProtocolError>;
pub fn decode_frame(bytes:&[u8], limits:&ProtocolLimits) -> Result<Message, ProtocolError>;
impl PeerNode {
    pub async fn bind(config:PeerConfig) -> Result<Self, TransportError>;
    pub async fn sync_objects(&self, peer:PeerAddress, roots:&[ObjectId]) -> Result<SyncReport, TransportError>;
    pub async fn propose(&self, authority:PeerAddress, request:ProposalEnvelope) -> Result<PromotionReply, TransportError>;
}
```
ProposalEnvelope and PromotionReply are bounded serializable wire DTOs owned by codec/protocol. Inject an application callback at peer service composition; CLI validates/converts proposals and invokes the ledger. Transport must not open the ledger or import its concrete implementation. Local-only identity creation is outside deterministic replay. Use strict individual signature verification and domain-separated signed bytes.

Tests first: real two-endpoint loopback transfer; wrong ALPN/version/peer authorization denied; oversized/truncated/unknown frame rejected before allocation; corrupted object denied; missing dependencies stay unavailable; duplicate same proposal returns original outcome; same request ID/different payload rejected; offline peer can compute but cannot promote conflicting state; no second writer on timeout; stale epoch/checkpoint replay rejected; fair control progress during bulk transfer; importer never executes downloaded code.

Steps:
1. Define `chronos/1` ALPN and big-endian u32 payload length plus u8 type. Initial caps:1MiB control,64KiB chunks,64MiB object,32 streams/peer and configurable queued-byte quotas. Validate before allocation and enforce deadlines.
2. Use Iroh Minimal preset with explicit loopback/private addresses. No N0 defaults/public discovery in tests. Verified API shape: EndpointAddr::new(id).with_ip_addr(address) or with_addrs(TransportAddr iterator), not with_ip_addrs. Initiator writes before waiting for a response so remote stream acceptance progresses.
3. Transfer content-addressed objects with integrity, dependency and schema validation. Transport completion is not durable application acceptance. Acknowledge durability explicitly after transaction commit.
4. Persist domain writer identity and authority epoch. Only that writer accepts promotions. Never let a peer become writer based on heartbeat timeout. Static authority is the released mode; proposal data can replicate freely. Imported/restored databases open replay/read-only until explicit activation fences the former dispatcher and proves current epoch. Keep activation credentials outside exported snapshots; two copies of an authority ID must not both dispatch.
5. Specify explicit handoff: quiesce admission/dispatch, resolve or preserve in-flight intents, copy durable state, old writer persists closed epoch, authorize new writer at next epoch. If provider cannot fence old in-flight requests, retain their operation IDs and unknown slots. Do not claim crash-available automatic handoff; consensus is a future mode.
6. Keep all state-changing messages out of 0-RTT. Authenticate peer, then authorize tenant/domain/object access. Redact credentials from peer exports and logs.
7. Implement malformed input fuzz harness and fault simulation at protocol abstraction. Do not claim a deterministic simulator exercises real QUIC internals. Run actual loopback integration separately.

Licence/prior-art: retain Iroh/crypto notices; consensus, content gossip and signatures are established. A signed checkpoint proves attribution under trust assumptions, not the truth of remote observations or Byzantine consensus.

Non-goals: homegrown Raft, public relay use by default, BFT, global total ordering, universal offline irreversible commits.

Definition of done: real private QUIC and application-level failure tests pass with no public service use, and:
```sh
cargo test -p chronos-protocol -p chronos-transport --locked
cargo test --test peer_protocol --features peer --locked
cargo test --workspace --all-targets --features peer --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```
