use attow_ledger::Authority;
use chronos_codec::ObjectId;

#[test]
fn decision_metadata_survives_reopen_and_inspection_without_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let decision = ObjectId([17; 32]);
    let mut authority = Authority::open(&path)?;
    let before = authority.checkpoint()?;
    assert_eq!(authority.decision_generation(decision)?, None);
    assert_eq!(authority.checkpoint()?, before);
    authority.advance_generation(decision, 0)?;
    assert_eq!(authority.decision_generation(decision)?, Some(0));
    authority.advance_generation(decision, 1)?;
    let after = authority.checkpoint()?;
    drop(authority);
    let reopened = Authority::open(&path)?;
    assert_eq!(reopened.decision_generation(decision)?, Some(1));
    assert_eq!(reopened.checkpoint()?, after);
    let inspected = Authority::inspect(&path)?;
    assert_eq!(inspected.decision_generation(decision)?, Some(1));
    assert_eq!(inspected.checkpoint()?, after);
    Ok(())
}
