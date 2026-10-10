use chronosos_reference::core::{Authority, Intent, Result};
use std::sync::{Arc, Barrier, Mutex};

fn main() -> Result<()> {
    let mut authority = Authority::open(":memory:")?;
    let mut genesis = authority.snapshot()?.fork();
    genesis.put("flight/42/seat/12A", b"available".to_vec());
    authority.promote(genesis)?;
    let snapshot = authority.snapshot()?;
    let authority = Arc::new(Mutex::new(authority));
    let barrier = Arc::new(Barrier::new(5));
    let handles: Vec<_> = (0..5)
        .map(|id| {
            let mut branch = snapshot.fork();
            let authority = authority.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                assert_eq!(
                    branch.get("flight/42/seat/12A"),
                    Some(b"available".to_vec())
                );
                branch.put("flight/42/seat/12A", b"booking_pending".to_vec());
                branch
                    .stage(Intent {
                        id: format!("candidate-{id}"),
                        slot: "user-1/trip-7/outbound-ticket".into(),
                        request: b"MOCK book flight 42 seat 12A".to_vec(),
                    })
                    .unwrap();
                barrier.wait();
                authority.lock().unwrap().promote(branch).is_ok()
            })
        })
        .collect();
    let winners = handles
        .into_iter()
        .map(|h| h.join().expect("worker panicked"))
        .filter(|w| *w)
        .count();
    let authority = authority.lock().unwrap();
    assert_eq!(winners, 1);
    assert_eq!(authority.effect_count()?, 1);
    authority.verify_chain()?;
    println!("branches=5 promoted=1 conflicts=4 pending_effects=1 external_calls=0");
    println!("Local intent chosen. No external seat has been booked.");
    Ok(())
}
