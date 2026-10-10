use chronosos_reference::core::{Authority, Error, Intent, Result};
use std::sync::{Arc, Barrier, Mutex};

fn test_database(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "chronos-reference-{label}-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn isolated_forks_and_disjoint_promotions() -> Result<()> {
    let mut a = Authority::open(":memory:")?;
    let base = a.snapshot()?;
    let mut x = base.fork();
    let mut y = base.fork();
    x.put("x", vec![1]);
    y.put("y", vec![2]);
    // An independent observer demonstrates isolation without adding a read
    // dependency on x to the otherwise disjoint y transaction.
    assert_eq!(base.fork().get("x"), None);
    a.promote(x)?;
    a.promote(y)?;
    let mut committed = a.snapshot()?.fork();
    assert_eq!(committed.get("x"), Some(vec![1]));
    assert_eq!(committed.get("y"), Some(vec![2]));
    a.verify_chain()?;
    Ok(())
}

#[test]
fn write_skew_is_rejected() -> Result<()> {
    let mut a = Authority::open(":memory:")?;
    let mut init = a.snapshot()?.fork();
    init.put("A", vec![1]);
    init.put("B", vec![1]);
    a.promote(init)?;
    let snap = a.snapshot()?;
    let mut x = snap.fork();
    let mut y = snap.fork();
    assert_eq!(x.get("B"), Some(vec![1]));
    assert_eq!(y.get("A"), Some(vec![1]));
    x.put("A", vec![0]);
    y.put("B", vec![0]);
    a.promote(x)?;
    assert!(matches!(a.promote(y), Err(Error::Conflict(_))));
    Ok(())
}

#[test]
fn missing_reads_and_tombstones_detect_aba() -> Result<()> {
    let mut a = Authority::open(":memory:")?;
    let mut old = a.snapshot()?.fork();
    assert_eq!(old.get("key"), None);
    let mut x = a.snapshot()?.fork();
    x.put("key", vec![1]);
    a.promote(x)?;
    let mut y = a.snapshot()?.fork();
    y.delete("key");
    a.promote(y)?;
    old.put("other", vec![9]);
    assert!(matches!(a.promote(old), Err(Error::Conflict(_))));
    Ok(())
}

#[test]
fn scan_rejects_phantom() -> Result<()> {
    let mut a = Authority::open(":memory:")?;
    let mut old = a.snapshot()?.fork();
    assert!(old.scan_prefix("seat/").is_empty());
    let mut new = a.snapshot()?.fork();
    new.put("seat/1", vec![1]);
    a.promote(new)?;
    old.put("seat/2", vec![1]);
    assert!(matches!(a.promote(old), Err(Error::Conflict(_))));
    Ok(())
}

#[test]
fn concurrent_branches_choose_one_effect() -> Result<()> {
    for _ in 0..20 {
        let mut a = Authority::open(":memory:")?;
        let mut init = a.snapshot()?.fork();
        init.put("seat", b"available".to_vec());
        a.promote(init)?;
        let s = a.snapshot()?;
        let a = Arc::new(Mutex::new(a));
        let barrier = Arc::new(Barrier::new(5));
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let a = a.clone();
                let barrier = barrier.clone();
                let mut b = s.fork();
                std::thread::spawn(move || {
                    b.get("seat");
                    b.put("seat", b"pending".to_vec());
                    b.stage(Intent {
                        id: format!("i{i}"),
                        slot: "one-business-action".into(),
                        request: vec![i],
                    })
                    .unwrap();
                    barrier.wait();
                    a.lock().unwrap().promote(b).is_ok()
                })
            })
            .collect();
        let wins = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|v| *v)
            .count();
        assert_eq!(wins, 1);
        assert_eq!(a.lock().unwrap().effect_count()?, 1);
    }
    Ok(())
}

#[test]
fn conflicting_effect_rolls_back_other_writes() -> Result<()> {
    let mut a = Authority::open(":memory:")?;
    let mut x = a.snapshot()?.fork();
    x.stage(Intent {
        id: "a".into(),
        slot: "slot".into(),
        request: vec![],
    })?;
    a.promote(x)?;
    let mut y = a.snapshot()?.fork();
    y.put("must-not-appear", vec![1]);
    y.stage(Intent {
        id: "b".into(),
        slot: "slot".into(),
        request: vec![],
    })?;
    assert!(matches!(a.promote(y), Err(Error::Conflict(_))));
    assert_eq!(a.snapshot()?.fork().get("must-not-appear"), None);
    Ok(())
}

#[test]
fn restart_keeps_ambiguous_effect_occupied() -> Result<()> {
    let path = std::env::temp_dir().join(format!(
        "chronos-reference-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let mut a = Authority::open(&path)?;
        let mut b = a.snapshot()?.fork();
        b.stage(Intent {
            id: "intent".into(),
            slot: "slot".into(),
            request: b"request".to_vec(),
        })?;
        a.promote(b)?;
        a.mark_dispatching("intent")?;
    }
    {
        let mut a = Authority::open(&path)?;
        assert_eq!(a.recover_ambiguous()?, 1);
        assert_eq!(a.effect_state("intent")?, "UNKNOWN");
        let mut b = a.snapshot()?.fork();
        b.stage(Intent {
            id: "second".into(),
            slot: "slot".into(),
            request: vec![],
        })?;
        assert!(matches!(a.promote(b), Err(Error::Conflict(_))));
        a.confirm("intent", b"provider-receipt")?;
        assert_eq!(a.effect_state("intent")?, "CONFIRMED");
        a.verify_chain()?;
    }
    std::fs::remove_file(path).expect("remove our closed test database");
    Ok(())
}

#[test]
fn identical_logical_run_has_identical_commit_hashes() -> Result<()> {
    fn run() -> Result<[u8; 32]> {
        let mut a = Authority::open(":memory:")?;
        let mut b = a.snapshot()?.fork();
        b.put("b", vec![2]);
        b.put("a", vec![1]);
        a.promote(b)?;
        a.verify_chain()
    }
    assert_eq!(run()?, run()?);
    Ok(())
}

#[test]
fn matching_versions_do_not_allow_cross_authority_promotion() -> Result<()> {
    let mut source = Authority::open(":memory:")?;
    let mut target = Authority::open(":memory:")?;
    let mut first = source.snapshot()?.fork();
    first.put("input", b"source-value".to_vec());
    source.promote(first)?;
    let mut second = target.snapshot()?.fork();
    second.put("input", b"different-target-value".to_vec());
    target.promote(second)?;

    // Both inputs have version 1; version validation alone cannot distinguish
    // the unrelated authorities or the different values actually observed.
    let mut branch = source.snapshot()?.fork();
    let observed = branch.get("input").unwrap();
    branch.put("derived", observed);
    assert!(matches!(target.promote(branch), Err(Error::Invalid(_))));
    assert_eq!(target.snapshot()?.fork().get("derived"), None);
    target.verify_chain()?;
    Ok(())
}

#[test]
fn same_authority_reopen_accepts_existing_branch() -> Result<()> {
    let path = test_database("reopen");
    let branch = {
        let a = Authority::open(&path)?;
        let mut branch = a.snapshot()?.fork();
        branch.put("value", b"survives-reopen".to_vec());
        branch
    };
    {
        let mut reopened = Authority::open(&path)?;
        reopened.promote(branch)?;
        assert_eq!(
            reopened.snapshot()?.fork().get("value"),
            Some(b"survives-reopen".to_vec())
        );
        reopened.verify_chain()?;
    }
    std::fs::remove_file(path).expect("remove our closed test database");
    Ok(())
}

#[test]
fn legacy_metadata_gains_stable_authority_identity() -> Result<()> {
    let path = test_database("migration");
    {
        let old = rusqlite::Connection::open(&path)?;
        old.execute_batch(
            "CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),epoch INTEGER NOT NULL,head BLOB NOT NULL);
             INSERT INTO meta VALUES(1,0,zeroblob(32));",
        )?;
    }
    let branch = {
        let migrated = Authority::open(&path)?;
        let mut branch = migrated.snapshot()?.fork();
        branch.put("after-migration", vec![1]);
        branch
    };
    {
        let mut reopened = Authority::open(&path)?;
        reopened.promote(branch)?;
        reopened.verify_chain()?;
    }
    std::fs::remove_file(path).expect("remove our closed test database");
    Ok(())
}

#[test]
fn independent_connections_atomically_choose_one_effect_per_slot() -> Result<()> {
    const ROUNDS: usize = 10;
    let path = test_database("connections");
    let first = Authority::open(&path)?;
    let second = Authority::open(&path)?;
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for (worker, mut authority) in [first, second].into_iter().enumerate() {
        let snapshot = authority.snapshot()?;
        let branches = (0..ROUNDS)
            .map(|round| {
                let mut branch = snapshot.fork();
                // Deliberately disjoint writes: the shared effect slot itself
                // must arbitrate, rather than an incidental key conflict.
                branch.put(&format!("winner/{round}/{worker}"), vec![1]);
                branch.stage(Intent {
                    id: format!("intent-{round}-{worker}"),
                    slot: format!("slot-{round}"),
                    request: vec![worker as u8],
                })?;
                Ok(branch)
            })
            .collect::<Result<Vec<_>>>()?;
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            branches
                .into_iter()
                .map(|branch| {
                    barrier.wait();
                    authority.promote(branch)
                })
                .collect::<Vec<_>>()
        }));
    }
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("promotion worker panicked"))
        .collect::<Vec<_>>();
    {
        let observer = Authority::open(&path)?;
        let mut committed = observer.snapshot()?.fork();
        for round in 0..ROUNDS {
            let mut wins = 0;
            for (worker, results) in outcomes.iter().enumerate() {
                match &results[round] {
                    Ok(_) => {
                        wins += 1;
                        assert_eq!(
                            committed.get(&format!("winner/{round}/{worker}")),
                            Some(vec![1])
                        );
                    }
                    Err(Error::Conflict(slot)) => {
                        assert_eq!(slot, &format!("slot-{round}"));
                        assert_eq!(committed.get(&format!("winner/{round}/{worker}")), None);
                    }
                    Err(other) => panic!("unexpected SQLite/validation failure: {other}"),
                }
            }
            assert_eq!(wins, 1);
        }
        assert_eq!(observer.effect_count()?, ROUNDS as i64);
        observer.verify_chain()?;
    }
    std::fs::remove_file(path).expect("remove our closed test database");
    Ok(())
}

#[test]
fn chain_verification_uses_consistent_snapshot_during_other_connection_commits() -> Result<()> {
    let path = test_database("verify");
    let mut writer = Authority::open(&path)?;
    let reader = Authority::open(&path)?;
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let worker = std::thread::spawn(move || -> Result<()> {
        writer_barrier.wait();
        for i in 0u64..100 {
            let mut branch = writer.snapshot()?.fork();
            branch.put("counter", i.to_le_bytes().to_vec());
            writer.promote(branch)?;
        }
        Ok(())
    });
    barrier.wait();
    let mut verification_error = None;
    for _ in 0..100 {
        if let Err(error) = reader.verify_chain() {
            verification_error = Some(error);
            break;
        }
    }
    worker.join().expect("writer panicked")?;
    if let Some(error) = verification_error {
        return Err(error);
    }
    reader.verify_chain()?;
    drop(reader);
    std::fs::remove_file(path).expect("remove our closed test database");
    Ok(())
}
