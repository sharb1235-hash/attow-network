use attow_local::owned_loopback::OwnedLoopbackFence;
use chronos_codec::ObjectId;
use chronos_ledger::{AuthorityCheckpoint, FenceRequest, WriterFence};
use chronos_state::DomainId;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const WRITER: ObjectId = ObjectId([9; 32]);
fn initial() -> AuthorityCheckpoint {
    AuthorityCheckpoint {
        domain: DomainId([1; 32]),
        writer: None,
        epoch: 1,
        head: ObjectId([2; 32]),
        next_writer: None,
    }
}
fn request(cp: AuthorityCheckpoint) -> FenceRequest {
    FenceRequest {
        checkpoint: cp,
        writer: WRITER,
        epoch: cp.epoch,
    }
}

#[test]
fn durable_anchor_excludes_other_handles_and_rejects_rollback() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("external-anchor");
    let cp = initial();
    let fence = OwnedLoopbackFence::enroll(&path, &cp, WRITER)?;
    let lease = fence.acquire(&request(cp))?;
    assert!(
        OwnedLoopbackFence::open(&path, WRITER)?
            .acquire(&request(cp))
            .is_err()
    );
    let next = AuthorityCheckpoint {
        writer: Some(WRITER),
        head: ObjectId([3; 32]),
        ..cp
    };
    lease.advance(&cp, &next)?;
    assert!(lease.check(&cp).is_err());
    lease.check(&next)?;
    drop(lease);
    drop(fence);
    let resumed = OwnedLoopbackFence::open(&path, WRITER)?;
    assert!(resumed.acquire(&request(cp)).is_err());
    resumed.acquire(&request(next))?.check(&next)?;
    assert!(OwnedLoopbackFence::open(&path, ObjectId([10; 32])).is_err());
    Ok(())
}

#[test]
fn missing_anchor_cannot_be_recreated_from_a_federated_copy() -> TestResult {
    let dir = tempfile::tempdir()?;
    let missing = dir.path().join("missing");
    assert!(OwnedLoopbackFence::open(&missing, WRITER).is_err());
    assert!(!missing.exists());
    let cp = AuthorityCheckpoint {
        writer: Some(WRITER),
        ..initial()
    };
    assert!(OwnedLoopbackFence::enroll(&missing, &cp, WRITER).is_err());
    assert!(!missing.exists());
    let path = dir.path().join("anchor");
    let _fence = OwnedLoopbackFence::enroll(&path, &initial(), WRITER)?;
    assert!(OwnedLoopbackFence::enroll(&path, &initial(), WRITER).is_err());
    Ok(())
}

#[test]
fn closed_epoch_or_foreign_domain_cannot_advance_demo_anchor() -> TestResult {
    let dir = tempfile::tempdir()?;
    let cp = initial();
    let fence = OwnedLoopbackFence::enroll(&dir.path().join("anchor"), &cp, WRITER)?;
    let lease = fence.acquire(&request(cp))?;
    let next = AuthorityCheckpoint {
        writer: Some(WRITER),
        ..cp
    };
    lease.advance(&cp, &next)?;
    for bad in [
        AuthorityCheckpoint {
            domain: DomainId([5; 32]),
            ..next
        },
        AuthorityCheckpoint { epoch: 2, ..next },
        AuthorityCheckpoint {
            next_writer: Some(ObjectId([6; 32])),
            ..next
        },
        AuthorityCheckpoint {
            writer: None,
            ..next
        },
    ] {
        assert!(lease.advance(&next, &bad).is_err());
    }
    lease.check(&next)?;
    Ok(())
}

#[test]
fn owned_lock_child() -> TestResult {
    let Some(path) = std::env::var_os("ATTOW_FENCE_TEST_CHILD") else {
        return Ok(());
    };
    let path = Path::new(&path);
    let fence = OwnedLoopbackFence::open(path, WRITER)?;
    let _lease = fence.acquire(&request(initial()))?;
    std::fs::write(path.join("ready"), b"locked")?;
    std::thread::sleep(Duration::from_secs(30));
    Ok(())
}

#[test]
fn cross_process_exclusion_survives_holder_termination() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("anchor");
    let fence = OwnedLoopbackFence::enroll(&path, &initial(), WRITER)?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "owned_lock_child", "--nocapture"])
        .env("ATTOW_FENCE_TEST_CHILD", &path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    while !path.join("ready").exists() {
        if child.try_wait()?.is_some() || started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not acquire the owned fence");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let excluded = fence.acquire(&request(initial())).is_err();
    child.kill()?;
    child.wait()?;
    assert!(excluded);
    fence.acquire(&request(initial()))?.check(&initial())?;
    Ok(())
}
