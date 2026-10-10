//! Original local provider fixture; never a real purchase integration.
use crate::{
    evidence::{EffectProvider, ProviderError, ProviderEvidence},
    gateway::DispatchAuthorization,
};
use chronos_codec::{Canonical, Limits, Object, ObjectId, Value, object_id};
use chronos_ledger::{DispatchTicket, EvidenceStatus, EvidenceVerifier, LedgerError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 64 * 1024;
const MAX_MODEL: usize = 8 * 1024 * 1024;
const APP_ID: i64 = 0x41545053;

pub struct Purchase {
    pub quantity: u64,
}
impl Purchase {
    pub fn encode(&self) -> Result<Vec<u8>, ProviderError> {
        if !(1..=1_000_000_000).contains(&self.quantity) {
            return Err(ProviderError::Configuration);
        }
        Object {
            refs: Default::default(),
            payload: Value::Array(vec![
                Value::Text("attow-sim-purchase".into()),
                Value::Unsigned(1),
                Value::Unsigned(self.quantity),
            ]),
        }
        .encode()
        .map_err(|_| ProviderError::Protocol)
    }
    fn decode(bytes: &[u8]) -> Result<Self, ProviderError> {
        if bytes.len() > 1024 {
            return Err(ProviderError::Limit);
        }
        let object =
            Object::decode(bytes, &Limits::default()).map_err(|_| ProviderError::Protocol)?;
        let Value::Array(fields) = object.payload else {
            return Err(ProviderError::Protocol);
        };
        let [
            Value::Text(tag),
            Value::Unsigned(1),
            Value::Unsigned(quantity),
        ] = fields.as_slice()
        else {
            return Err(ProviderError::Protocol);
        };
        if tag != "attow-sim-purchase"
            || !object.refs.is_empty()
            || !(1..=1_000_000_000).contains(quantity)
        {
            return Err(ProviderError::Protocol);
        }
        Ok(Self {
            quantity: *quantity,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulatorConfig {
    pub provider: String,
    pub inventory: u64,
    pub hold_ttl: u64,
    pub idempotency_retention: u64,
}
impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            provider: "attow-owned-simulator".into(),
            inventory: 10,
            hold_ttl: 10,
            idempotency_retention: 100,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fault {
    None,
    CommitThenDisconnect,
    RejectNext,
    HideNextLookup,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatorStats {
    pub inventory: u64,
    pub held: u64,
    pub accepted: u64,
    pub dispatch_requests: u64,
    pub lookups: u64,
}

pub struct SimulatorServer {
    client: SimulatorClient,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
#[derive(Clone)]
pub struct SimulatorController {
    client: SimulatorClient,
}
#[derive(Clone)]
pub struct SimulatorClient {
    address: SocketAddr,
    key: [u8; 32],
    contract: ObjectId,
    provider: String,
}
impl SimulatorServer {
    pub fn start(path: &Path, config: &SimulatorConfig) -> Result<Self, ProviderError> {
        if config.provider.is_empty()
            || config.provider.len() > 64
            || !config
                .provider
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || config.inventory > 1_000_000_000
            || !(1..=1_000_000).contains(&config.hold_ttl)
            || !(1..=1_000_000).contains(&config.idempotency_retention)
        {
            return Err(ProviderError::Configuration);
        }
        let mut connection = Connection::open(path).map_err(storage)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(storage)?;
        let application: i64 = connection
            .query_row("PRAGMA application_id", [], |row| row.get(0))
            .map_err(storage)?;
        let tables:i64=connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|row|row.get(0)).map_err(storage)?;
        if application != APP_ID && (application != 0 || tables != 0) {
            return Err(ProviderError::Configuration);
        }
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(storage)?;
        connection
            .set_limit(
                rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
                (MAX_MODEL + 1024) as i32,
            )
            .map_err(storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        transaction
            .pragma_update(None, "application_id", APP_ID)
            .map_err(storage)?;
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS simulator(id INTEGER PRIMARY KEY CHECK(id=1),model BLOB NOT NULL)").map_err(storage)?;
        let present: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM simulator WHERE id=1)",
                [],
                |row| row.get(0),
            )
            .map_err(storage)?;
        let model = if present {
            load(&transaction)?
        } else {
            let mut key = [0; 32];
            getrandom::fill(&mut key).map_err(|_| ProviderError::Unavailable)?;
            Model {
                schema: 1,
                config: config.clone(),
                key,
                tick: 0,
                inventory: config.inventory,
                holds: BTreeMap::new(),
                operations: BTreeMap::new(),
                dispatch_requests: 0,
                lookups: 0,
                fault: Fault::None,
            }
        };
        if model.config != *config || model.schema != 1 {
            return Err(ProviderError::Configuration);
        }
        save(&transaction, &model)?;
        transaction.commit().map_err(storage)?;
        let listener =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(unavailable)?;
        let address = listener.local_addr().map_err(unavailable)?;
        let client = SimulatorClient {
            address,
            key: model.key,
            contract: contract(&model)?,
            provider: config.provider.clone(),
        };
        let worker_client = client.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("attow-provider-simulator".into())
            .spawn(move || {
                while let Ok((mut stream, _)) = listener.accept() {
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    // Malformed/unauthorized requests are dropped; no business transition occurs.
                    let _result = serve(&mut connection, &mut stream, &worker_client);
                }
            })
            .map_err(unavailable)?;
        Ok(Self {
            client,
            stop,
            thread: Some(thread),
        })
    }
    pub fn controller(&self) -> SimulatorController {
        SimulatorController {
            client: self.client.clone(),
        }
    }
    pub fn connect(&self) -> SimulatorClient {
        self.client.clone()
    }
    pub fn contract_id(&self) -> ObjectId {
        self.client.contract
    }
}
impl Drop for SimulatorServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // A loopback wakeup releases blocking accept; the worker checks stop
        // before reading a frame. Active reads have a two-second deadline.
        let _wake = TcpStream::connect_timeout(&self.client.address, Duration::from_secs(2));
        if let Some(worker) = self.thread.take() {
            let _joined = worker.join();
        }
    }
}
impl SimulatorController {
    pub fn hold(&self, operation: ObjectId, quantity: u64) -> Result<(), ProviderError> {
        self.client
            .command(Command::Hold {
                operation: operation.to_string(),
                quantity,
            })
            .and_then(ok_reply)
    }
    pub fn advance_ticks(&self, ticks: u64) -> Result<(), ProviderError> {
        self.client
            .command(Command::Advance { ticks })
            .and_then(ok_reply)
    }
    pub fn stats(&self) -> Result<SimulatorStats, ProviderError> {
        match self.client.command(Command::Stats)? {
            Reply::Stats(stats) => Ok(stats),
            _ => Err(ProviderError::Protocol),
        }
    }
    pub fn set_fault(&self, fault: Fault) -> Result<(), ProviderError> {
        self.client
            .command(Command::Fault { fault })
            .and_then(ok_reply)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    domain: [u8; 32],
    epoch: u64,
    intent: [u8; 32],
    attempt: u64,
    contract: [u8; 32],
    provider: String,
    operation: [u8; 32],
    payload: [u8; 32],
}
impl Binding {
    fn from_ticket(ticket: &DispatchTicket) -> Self {
        Self {
            domain: ticket.domain().0,
            epoch: ticket.epoch(),
            intent: ticket.intent().0,
            attempt: ticket.attempt(),
            contract: ticket.contract().0,
            provider: ticket.provider().into(),
            operation: ticket.operation().0,
            payload: ticket.payload().0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hold {
    quantity: u64,
    expires: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    binding: Binding,
    status: u8,
    tick: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    schema: u16,
    config: SimulatorConfig,
    key: [u8; 32],
    tick: u64,
    inventory: u64,
    holds: BTreeMap<String, Hold>,
    operations: BTreeMap<String, Operation>,
    dispatch_requests: u64,
    lookups: u64,
    fault: Fault,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Command {
    Stats,
    Hold { operation: String, quantity: u64 },
    Advance { ticks: u64 },
    Fault { fault: Fault },
    Dispatch { binding: Binding, body: Vec<u8> },
    Reconcile { binding: Binding },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u16,
    nonce: [u8; 16],
    command: Command,
    mac: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u16,
    binding: Binding,
    status: u8,
    tick: u64,
    mac: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Reply {
    Ok,
    Stats(SimulatorStats),
    Evidence(Box<Receipt>),
    Error(u8),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u16,
    nonce: [u8; 16],
    reply: Reply,
    mac: [u8; 32],
}
fn storage(_: rusqlite::Error) -> ProviderError {
    ProviderError::Storage
}
fn unavailable(_: std::io::Error) -> ProviderError {
    ProviderError::Unavailable
}
fn mac<T: Serialize>(key: &[u8; 32], tag: &str, payload: &T) -> Result<[u8; 32], ProviderError> {
    let bytes = serde_json::to_vec(&(tag, 1, payload)).map_err(|_| ProviderError::Protocol)?;
    if bytes.len() > MAX_FRAME {
        return Err(ProviderError::Limit);
    }
    Ok(*blake3::keyed_hash(key, &bytes).as_bytes())
}
fn authentic(expected: [u8; 32], actual: [u8; 32]) -> bool {
    blake3::Hash::from(expected) == blake3::Hash::from(actual)
}
fn contract(model: &Model) -> Result<ObjectId, ProviderError> {
    let bytes = serde_json::to_vec(&(
        "attow-owned-provider-contract",
        1,
        &model.config,
        blake3::hash(&model.key).as_bytes(),
    ))
    .map_err(|_| ProviderError::Protocol)?;
    Ok(object_id(0x4150, 1, &bytes))
}
fn load(connection: &Connection) -> Result<Model, ProviderError> {
    let bytes: Option<Vec<u8>> = connection
        .query_row(
            "SELECT CASE WHEN length(model)<=?1 THEN model ELSE NULL END FROM simulator WHERE id=1",
            [MAX_MODEL as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?
        .flatten();
    let model: Model = serde_json::from_slice(&bytes.ok_or(ProviderError::Storage)?)
        .map_err(|_| ProviderError::Storage)?;
    if model.schema != 1
        || model.operations.len() > 4096
        || model.holds.len() > 4096
        || model.inventory > model.config.inventory
    {
        return Err(ProviderError::Storage);
    }
    Ok(model)
}
fn save(connection: &Connection, model: &Model) -> Result<(), ProviderError> {
    let bytes = serde_json::to_vec(model).map_err(|_| ProviderError::Storage)?;
    if bytes.len() > MAX_MODEL {
        return Err(ProviderError::Limit);
    }
    connection.execute("INSERT INTO simulator(id,model) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET model=excluded.model",[bytes]).map_err(storage)?;
    Ok(())
}
fn read_frame<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> Result<T, ProviderError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut header = [0; 4];
    read_bounded(stream, &mut header, deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(ProviderError::Limit);
    }
    let mut bytes = vec![0; length];
    read_bounded(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|_| ProviderError::Protocol)
}
fn read_bounded(
    stream: &mut TcpStream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> Result<(), ProviderError> {
    while !buffer.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .ok_or(ProviderError::Unavailable)?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(unavailable)?;
        match stream.read(buffer) {
            Ok(0) => return Err(ProviderError::Unavailable),
            Ok(count) => buffer = &mut buffer[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(unavailable(error)),
        }
    }
    Ok(())
}
fn write_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), ProviderError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ProviderError::Protocol)?;
    if bytes.len() > MAX_FRAME {
        return Err(ProviderError::Limit);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    write_bounded(
        stream,
        &(bytes.len() as u32).to_be_bytes(),
        deadline,
        |stream, remaining| stream.set_write_timeout(Some(remaining)),
    )?;
    write_bounded(stream, &bytes, deadline, |stream, remaining| {
        stream.set_write_timeout(Some(remaining))
    })?;
    Ok(())
}
fn write_bounded<W: Write>(
    writer: &mut W,
    mut bytes: &[u8],
    deadline: Instant,
    mut set_timeout: impl FnMut(&mut W, Duration) -> std::io::Result<()>,
) -> Result<(), ProviderError> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .ok_or(ProviderError::Unavailable)?;
        set_timeout(writer, remaining).map_err(unavailable)?;
        match writer.write(bytes) {
            Ok(0) => return Err(ProviderError::Unavailable),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(unavailable(error)),
        }
    }
    if Instant::now() > deadline {
        return Err(ProviderError::Unavailable);
    }
    Ok(())
}
fn ok_reply(reply: Reply) -> Result<(), ProviderError> {
    if matches!(reply, Reply::Ok) {
        Ok(())
    } else {
        Err(ProviderError::Protocol)
    }
}
impl SimulatorClient {
    fn command(&self, command: Command) -> Result<Reply, ProviderError> {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| ProviderError::Unavailable)?;
        let signature = mac(&self.key, "attow-sim-request", &(nonce, &command))?;
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(2))
            .map_err(unavailable)?;
        write_frame(
            &mut stream,
            &Request {
                version: 1,
                nonce,
                command,
                mac: signature,
            },
        )?;
        let response: Response = read_frame(&mut stream)?;
        if response.version != 1
            || response.nonce != nonce
            || !authentic(
                mac(&self.key, "attow-sim-response", &(nonce, &response.reply))?,
                response.mac,
            )
        {
            return Err(ProviderError::InvalidEvidence);
        }
        match response.reply {
            Reply::Error(1) => Err(ProviderError::RetentionExpired),
            Reply::Error(2) => Err(ProviderError::InvalidAuthorization),
            Reply::Error(3) => Err(ProviderError::Limit),
            Reply::Error(_) => Err(ProviderError::Storage),
            reply => Ok(reply),
        }
    }
    fn send(
        &self,
        auth: &DispatchAuthorization<'_>,
        dispatch: bool,
    ) -> Result<ProviderEvidence, ProviderError> {
        let ticket = auth.ticket();
        if ticket.may_dispatch() != dispatch
            || ticket.contract() != self.contract
            || ticket.provider() != self.provider
            || ticket.payload() != object_id(0x4244, 1, auth.body())
        {
            return Err(ProviderError::InvalidAuthorization);
        }
        Purchase::decode(auth.body())?;
        let binding = Binding::from_ticket(ticket);
        let command = if dispatch {
            Command::Dispatch {
                binding,
                body: auth.body().to_vec(),
            }
        } else {
            Command::Reconcile { binding }
        };
        match self.command(command)? {
            Reply::Evidence(receipt) => ProviderEvidence::from_bytes(
                serde_json::to_vec(&receipt).map_err(|_| ProviderError::Protocol)?,
            ),
            _ => Err(ProviderError::Protocol),
        }
    }
}
impl EffectProvider for SimulatorClient {
    fn preflight(&self, proposal: &chronos_ledger::EffectProposal) -> Result<(), ProviderError> {
        if proposal.contract != self.contract || proposal.provider != self.provider {
            return Err(ProviderError::InvalidAuthorization);
        }
        let purchase = Purchase::decode(&proposal.body)?;
        if purchase.quantity != proposal.cost {
            return Err(ProviderError::InvalidAuthorization);
        }
        Ok(())
    }
    fn dispatch(
        &mut self,
        auth: &DispatchAuthorization<'_>,
    ) -> Result<ProviderEvidence, ProviderError> {
        self.send(auth, true)
    }
    fn reconcile(
        &mut self,
        auth: &DispatchAuthorization<'_>,
    ) -> Result<ProviderEvidence, ProviderError> {
        self.send(auth, false)
    }
}
impl EvidenceVerifier for SimulatorClient {
    fn contract_id(&self) -> ObjectId {
        self.contract
    }
    fn verify(&self, ticket: &DispatchTicket, bytes: &[u8]) -> Result<EvidenceStatus, LedgerError> {
        if ticket.contract() != self.contract || ticket.provider() != self.provider {
            return Err(LedgerError::InvalidEvidence);
        }
        if bytes.is_empty() {
            return Ok(EvidenceStatus::Unknown);
        }
        if bytes.len() > crate::evidence::MAX_PROVIDER_EVIDENCE {
            return Err(LedgerError::InvalidEvidence);
        }
        let receipt: Receipt =
            serde_json::from_slice(bytes).map_err(|_| LedgerError::InvalidEvidence)?;
        let expected = mac(
            &self.key,
            "attow-sim-receipt",
            &(
                receipt.version,
                &receipt.binding,
                receipt.status,
                receipt.tick,
            ),
        )
        .map_err(|_| LedgerError::InvalidEvidence)?;
        if receipt.version != 1
            || receipt.binding != Binding::from_ticket(ticket)
            || !authentic(expected, receipt.mac)
        {
            return Err(LedgerError::InvalidEvidence);
        }
        match receipt.status {
            0 => Ok(EvidenceStatus::Unknown),
            1 => Ok(EvidenceStatus::Confirmed),
            2 => Ok(EvidenceStatus::DefinitivelyRejected),
            _ => Err(LedgerError::InvalidEvidence),
        }
    }
}
fn held(model: &Model) -> u64 {
    model
        .holds
        .iter()
        .filter(|(key, hold)| hold.expires > model.tick && !model.operations.contains_key(*key))
        .map(|(_, hold)| hold.quantity)
        .sum()
}
fn receipt(model: &Model, binding: Binding, status: u8, tick: u64) -> Result<Reply, ProviderError> {
    let signature = mac(
        &model.key,
        "attow-sim-receipt",
        &(1_u16, &binding, status, tick),
    )?;
    Ok(Reply::Evidence(Box::new(Receipt {
        version: 1,
        binding,
        status,
        tick,
        mac: signature,
    })))
}
fn process(model: &mut Model, command: Command) -> Result<(Reply, bool), ProviderError> {
    let mut disconnect = false;
    let reply = match command {
        Command::Stats => Reply::Stats(SimulatorStats {
            inventory: model.inventory,
            held: held(model),
            accepted: model
                .operations
                .values()
                .filter(|op| op.status == 1)
                .count() as u64,
            dispatch_requests: model.dispatch_requests,
            lookups: model.lookups,
        }),
        Command::Advance { ticks } => {
            model.tick = model.tick.checked_add(ticks).ok_or(ProviderError::Limit)?;
            Reply::Ok
        }
        Command::Fault { fault } => {
            model.fault = fault;
            Reply::Ok
        }
        Command::Hold {
            operation,
            quantity,
        } => {
            if operation.len() != 64
                || !operation.bytes().all(|b| b.is_ascii_hexdigit())
                || quantity == 0
                || model.holds.contains_key(&operation)
                || model.operations.contains_key(&operation)
                || quantity > model.inventory.saturating_sub(held(model))
            {
                return Err(ProviderError::InvalidAuthorization);
            }
            if model.holds.len() >= 4096 {
                return Err(ProviderError::Limit);
            }
            model.holds.insert(
                operation,
                Hold {
                    quantity,
                    expires: model
                        .tick
                        .checked_add(model.config.hold_ttl)
                        .ok_or(ProviderError::Limit)?,
                },
            );
            Reply::Ok
        }
        Command::Dispatch { binding, body } => {
            if binding.contract != contract(model)?.0
                || binding.provider != model.config.provider
                || binding.payload != object_id(0x4244, 1, &body).0
            {
                return Err(ProviderError::InvalidAuthorization);
            }
            let purchase = Purchase::decode(&body)?;
            let key = ObjectId(binding.operation).to_string();
            model.dispatch_requests = model
                .dispatch_requests
                .checked_add(1)
                .ok_or(ProviderError::Limit)?;
            if let Some(previous) = model.operations.get(&key) {
                if previous.binding != binding {
                    return Err(ProviderError::InvalidAuthorization);
                }
                if model.tick.saturating_sub(previous.tick) >= model.config.idempotency_retention {
                    Reply::Error(1)
                } else {
                    receipt(model, binding, previous.status, previous.tick)?
                }
            } else {
                if model.operations.len() >= 4096 {
                    return Err(ProviderError::Limit);
                }
                let hold = model.holds.get(&key);
                let own = hold
                    .filter(|h| h.expires > model.tick)
                    .map_or(0, |h| h.quantity);
                let available = model
                    .inventory
                    .saturating_sub(held(model).saturating_sub(own));
                let reject = matches!(model.fault, Fault::RejectNext)
                    || purchase.quantity > available
                    || hold.is_some_and(|h| {
                        h.expires <= model.tick || h.quantity != purchase.quantity
                    });
                let status = if reject { 2 } else { 1 };
                if status == 1 {
                    model.inventory -= purchase.quantity;
                }
                disconnect = matches!(model.fault, Fault::CommitThenDisconnect);
                if matches!(model.fault, Fault::CommitThenDisconnect | Fault::RejectNext) {
                    model.fault = Fault::None;
                }
                model.operations.insert(
                    key,
                    Operation {
                        binding: binding.clone(),
                        status,
                        tick: model.tick,
                    },
                );
                receipt(model, binding, status, model.tick)?
            }
        }
        Command::Reconcile { binding } => {
            if binding.contract != contract(model)?.0 || binding.provider != model.config.provider {
                return Err(ProviderError::InvalidAuthorization);
            }
            model.lookups = model.lookups.checked_add(1).ok_or(ProviderError::Limit)?;
            let key = ObjectId(binding.operation).to_string();
            if matches!(model.fault, Fault::HideNextLookup) {
                model.fault = Fault::None;
                receipt(model, binding, 0, model.tick)?
            } else if let Some(previous) = model.operations.get(&key) {
                if previous.binding != binding {
                    return Err(ProviderError::InvalidAuthorization);
                }
                if model.tick.saturating_sub(previous.tick) >= model.config.idempotency_retention {
                    Reply::Error(1)
                } else {
                    receipt(model, binding, previous.status, previous.tick)?
                }
            } else {
                receipt(model, binding, 0, model.tick)?
            }
        }
    };
    Ok((reply, disconnect))
}
fn serve(
    connection: &mut Connection,
    stream: &mut TcpStream,
    client: &SimulatorClient,
) -> Result<(), ProviderError> {
    let request: Request = read_frame(stream)?;
    if request.version != 1
        || !authentic(
            mac(
                &client.key,
                "attow-sim-request",
                &(request.nonce, &request.command),
            )?,
            request.mac,
        )
    {
        return Err(ProviderError::InvalidAuthorization);
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    let mut model = load(&transaction)?;
    if model.key != client.key || contract(&model)? != client.contract {
        return Err(ProviderError::Storage);
    }
    let (reply, disconnect) = match process(&mut model, request.command) {
        Ok(result) => {
            save(&transaction, &model)?;
            transaction.commit().map_err(storage)?;
            result
        }
        Err(error) => {
            transaction.rollback().map_err(storage)?;
            (
                Reply::Error(match error {
                    ProviderError::InvalidAuthorization => 2,
                    ProviderError::Limit => 3,
                    _ => 4,
                }),
                false,
            )
        }
    };
    if disconnect {
        return Ok(());
    }
    let signature = mac(&client.key, "attow-sim-response", &(request.nonce, &reply))?;
    write_frame(
        stream,
        &Response {
            version: 1,
            nonce: request.nonce,
            reply,
            mac: signature,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(server: &SimulatorServer, body: &[u8]) -> Binding {
        Binding {
            domain: [1; 32],
            epoch: 1,
            intent: [2; 32],
            attempt: 1,
            contract: server.contract_id().0,
            provider: SimulatorConfig::default().provider,
            operation: [3; 32],
            payload: object_id(0x4244, 1, body).0,
        }
    }
    #[test]
    fn actual_duplicate_requests_do_not_duplicate_inventory_and_binding_cannot_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let server = SimulatorServer::start(
            &dir.path().join("server.sqlite"),
            &SimulatorConfig::default(),
        )?;
        let body = Purchase { quantity: 2 }.encode()?;
        let binding = binding(&server, &body);
        for _ in 0..2 {
            let reply = server.client.command(Command::Dispatch {
                binding: binding.clone(),
                body: body.clone(),
            })?;
            assert!(matches!(reply,Reply::Evidence(receipt) if receipt.status==1));
        }
        let stats = server.controller().stats()?;
        assert_eq!(stats.accepted, 1);
        assert_eq!(stats.inventory, 8);
        assert_eq!(stats.dispatch_requests, 2);
        let mut altered = binding;
        altered.epoch = 2;
        assert!(
            server
                .client
                .command(Command::Dispatch {
                    binding: altered,
                    body
                })
                .is_err()
        );
        assert_eq!(server.controller().stats()?.inventory, 8);
        Ok(())
    }
    #[test]
    fn unauthenticated_and_oversized_requests_cannot_mutate_provider_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let server = SimulatorServer::start(
            &dir.path().join("server.sqlite"),
            &SimulatorConfig::default(),
        )?;
        let mut invalid = server.connect();
        invalid.key = [0; 32];
        assert!(invalid.command(Command::Advance { ticks: 10 }).is_err());
        let mut stream = TcpStream::connect(server.client.address)?;
        stream.write_all(&u32::MAX.to_be_bytes())?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte)?, 0);
        assert_eq!(server.controller().stats()?.accepted, 0);
        Ok(())
    }
    #[test]
    fn slowly_dripped_header_has_an_absolute_frame_deadline()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let server = SimulatorServer::start(
            &dir.path().join("server.sqlite"),
            &SimulatorConfig::default(),
        )?;
        let mut stream = TcpStream::connect(server.client.address)?;
        for _ in 0..3 {
            stream.write_all(&[0])?;
            thread::sleep(Duration::from_millis(800));
        }
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        let mut byte = [0];
        assert_eq!(
            stream.read(&mut byte)?,
            0,
            "slow request retained the only server worker beyond its absolute deadline"
        );
        assert_eq!(server.controller().stats()?.accepted, 0);
        Ok(())
    }
    #[test]
    fn slow_partial_writes_cannot_restart_the_absolute_deadline() {
        struct SlowWriter {
            bytes: usize,
        }
        impl Write for SlowWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                thread::sleep(Duration::from_millis(40));
                self.bytes += 1;
                Ok(1)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut writer = SlowWriter { bytes: 0 };
        let result = write_bounded(
            &mut writer,
            &[1, 2, 3, 4],
            Instant::now() + Duration::from_millis(70),
            |_, _| Ok(()),
        );
        assert_eq!(result, Err(ProviderError::Unavailable));
        assert!(
            writer.bytes < 4,
            "partial writes restarted the entire time budget"
        );
    }
}
