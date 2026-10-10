use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use iroh::{Endpoint, endpoint::presets};
use std::time::Duration;
use wasmtime::{Config, Engine, Instance, Module, Store, StoreLimits, StoreLimitsBuilder};

struct State { limits: StoreLimits }

fn runtime_probe() -> Result<()> {
    let mut config = Config::new();
    config.consume_fuel(true)
        .cranelift_nan_canonicalization(true)
        .wasm_relaxed_simd(false)
        .wasm_simd(false)
        .wasm_component_model(true)
        .memory_init_cow(true);
    let engine = Engine::new(&config)?;
    let mut store = Store::new(&engine, State {
        limits: StoreLimitsBuilder::new().memory_size(65536).instances(2).build()
    });
    store.limiter(|s| &mut s.limits);
    store.set_fuel(10000)?;
    let module = Module::new(&engine, r#"(module
      (memory (export "memory") 1 1)
      (func (export "add") (param i32 i32) (result i32)
        local.get 0 local.get 1 i32.add)
      (func (export "spin") (loop br 0)))"#)?;
    let instance = Instance::new(&mut store, &module, &[])?;
    let add = instance.get_typed_func::<(i32, i32), i32>(&mut store, "add")?;
    ensure!(add.call(&mut store, (20, 22))? == 42);
    let spin = instance.get_typed_func::<(), ()>(&mut store, "spin")?;
    store.set_fuel(100)?;
    let error = spin.call(&mut store, ()).expect_err("finite fuel must trap");
    ensure!(matches!(error.downcast_ref::<wasmtime::Trap>(), Some(wasmtime::Trap::OutOfFuel)));
    println!("wasmtime core: fixed-memory call=42; fuel limit traps");

    // A trapped Store is not a clean retry boundary. Share Engine/compiled code,
    // but create fresh execution state for this independent component attempt.
    let mut store = Store::new(&engine, State {
        limits: StoreLimitsBuilder::new().memory_size(65536).instances(2).build()
    });
    store.limiter(|s| &mut s.limits);

    let component = wasmtime::component::Component::new(&engine, r#"(component
      (core module $m
        (func (export "add") (param i32 i32) (result i32)
          local.get 0 local.get 1 i32.add))
      (core instance $i (instantiate $m))
      (func (export "add") (param "a" u32) (param "b" u32) (result u32)
        (canon lift (core func $i "add"))))"#)?;
    let linker = wasmtime::component::Linker::<State>::new(&engine);
    store.set_fuel(10000)?;
    let instance = linker.instantiate(&mut store, &component)?;
    let add = instance.get_typed_func::<(u32, u32), (u32,)>(&mut store, "add")?;
    let (sum,) = add.call(&mut store, (19, 23))?;
    ensure!(sum == 42);
    println!("wasmtime component: typed call=42; automatic post-return");
    Ok(())
}

fn data_probe() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut db = rusqlite::Connection::open(dir.path().join("ledger.sqlite"))?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "synchronous", "FULL")?;
    let mode: String = db.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    ensure!(mode == "wal");
    let mut encoder = minicbor::Encoder::new(Vec::new());
    encoder.array(3)?.str("chronos/probe/v1")?.u64(7)?.bytes(b"state")?;
    let bytes = encoder.into_writer();
    let digest = blake3::hash(&bytes);
    // Fixed secret is ONLY a reproducible test vector. Production uses OS entropy.
    let key = SigningKey::from_bytes(&[19; 32]);
    let signature = key.sign(&bytes);
    key.verifying_key().verify_strict(&bytes, &signature)?;
    db.execute_batch("CREATE TABLE event(hash BLOB PRIMARY KEY, body BLOB NOT NULL) WITHOUT ROWID;")?;
    let tx = db.transaction()?;
    tx.execute("INSERT INTO event(hash, body) VALUES(?1, ?2)", rusqlite::params![digest.as_bytes(), bytes])?;
    tx.commit()?;
    let mut snapshot = rusqlite::Connection::open(dir.path().join("snapshot.sqlite"))?;
    {
        let backup = rusqlite::backup::Backup::new(&db, &mut snapshot)?;
        backup.run_to_completion(64, Duration::from_millis(1), None)?;
    }
    let count: i64 = snapshot.query_row("SELECT count(*) FROM event", [], |r| r.get(0))?;
    ensure!(count == 1);
    let root = rpds::HashTrieMapSync::<String, u64>::new_sync().insert("x".into(), 1);
    let branch = root.insert("x".into(), 2);
    ensure!(root.get("x") == Some(&1) && branch.get("x") == Some(&2));
    println!("data: SQLite {} WAL commit + online backup; explicit CBOR+BLAKE3+strict Ed25519; persistent map fork", rusqlite::version());
    Ok(())
}

async fn transport_probe() -> Result<()> {
    const ALPN: &[u8] = b"chronos-dep-probe/1";
    let server = Endpoint::builder(presets::Minimal)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")?
        .alpns(vec![ALPN.to_vec()])
        .bind().await?;
    let client = Endpoint::builder(presets::Minimal)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")?
        .bind().await?;
    let socket = *server.bound_sockets().first().context("no server socket")?;
    let addr = iroh::EndpointAddr::new(server.id()).with_ip_addr(socket);
    let server_task = tokio::spawn({
        let server = server.clone();
        async move {
            let conn = server.accept().await.context("accept ended")?.await?;
            let (mut send, mut recv) = conn.accept_bi().await?;
            let data = recv.read_to_end(1024).await?;
            send.write_all(&data).await?;
            send.finish()?;
            conn.closed().await;
            Ok::<_, anyhow::Error>(())
        }
    });
    let conn = client.connect(addr, ALPN).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    send.write_all(b"chronos") .await?;
    send.finish()?;
    ensure!(recv.read_to_end(1024).await? == b"chronos");
    conn.close(0u32.into(), b"done");
    server_task.await??;
    client.close().await;
    server.close().await;
    println!("iroh: Minimal preset direct loopback bidirectional QUIC echo; endpoints closed");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    runtime_probe()?;
    data_probe()?;
    tokio::time::timeout(Duration::from_secs(20), transport_probe()).await??;
    Ok(())
}
