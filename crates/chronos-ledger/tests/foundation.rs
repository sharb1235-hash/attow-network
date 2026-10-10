use attow_ledger::Authority;

#[test]
fn persistent_authority_identity_is_random_and_survives_reopen()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.db");
    let authority = Authority::open(&path)?;
    let domain = authority.domain();
    assert_ne!(domain.0, [0; 32]);
    drop(authority);
    assert_eq!(Authority::open(&path)?.domain(), domain);
    assert_ne!(
        Authority::open(&directory.path().join("other.db"))?.domain(),
        domain
    );
    Ok(())
}

#[test]
fn initial_managed_root_is_in_authority_domain() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let authority = Authority::open(&directory.path().join("authority.db"))?;
    assert_eq!(authority.current_root()?.domain(), authority.domain());
    assert_eq!(authority.current_root()?.sequence(), 0);
    Ok(())
}
