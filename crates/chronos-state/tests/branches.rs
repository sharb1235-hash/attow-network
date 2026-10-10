use attow_state::{Dependency, DomainId, Mutation, StateRoot, Version, fork};
use std::collections::BTreeMap;

#[test]
fn sibling_and_parent_are_isolated() -> Result<(), Box<dyn std::error::Error>> {
    let domain = DomainId([1; 32]);
    let root = StateRoot::new(domain);
    let mut left = fork(domain, &root, [1; 32])?;
    let mut right = fork(domain, &root, [2; 32])?;
    left.put(b"key".to_vec(), b"left".to_vec())?;
    assert_eq!(left.get(b"key")?, Some(b"left".to_vec()));
    assert_eq!(right.get(b"key")?, None);
    assert_eq!(root.get(b"key")?, None);
    Ok(())
}

#[test]
fn absent_and_blind_dependencies_survive_freeze() -> Result<(), Box<dyn std::error::Error>> {
    let root = StateRoot::new(DomainId([1; 32]));
    let mut branch = fork(DomainId([1; 32]), &root, [0; 32])?;
    branch.get(b"absent")?;
    branch.put(b"blind".to_vec(), vec![1])?;
    let frozen = branch.freeze()?;
    assert_eq!(
        frozen.dependencies(),
        &[
            Dependency::Key {
                key: b"absent".to_vec(),
                version: Version(0)
            },
            Dependency::Key {
                key: b"blind".to_vec(),
                version: Version(0)
            }
        ]
    );
    Ok(())
}

#[test]
fn tombstones_prevent_absence_aba() -> Result<(), Box<dyn std::error::Error>> {
    let root = StateRoot::new(DomainId([1; 32]));
    let present = root.apply(&BTreeMap::from([(b"key".to_vec(), Mutation::Put(vec![1]))]))?;
    let absent = present.apply(&BTreeMap::from([(b"key".to_vec(), Mutation::Delete)]))?;
    assert_eq!(root.version(b"key")?, Version(0));
    assert_eq!(absent.get(b"key")?, None);
    assert!(absent.version(b"key")? > present.version(b"key")?);
    Ok(())
}

#[test]
fn scan_tracks_namespace_and_merges_overlay() -> Result<(), Box<dyn std::error::Error>> {
    let domain = DomainId([1; 32]);
    let root = StateRoot::new(domain).apply(&BTreeMap::from([
        (b"p/a".to_vec(), Mutation::Put(vec![1])),
        (b"p/b".to_vec(), Mutation::Put(vec![2])),
    ]))?;
    let mut branch = fork(domain, &root, [3; 32])?;
    branch.delete(b"p/a")?;
    branch.put(b"p/c".to_vec(), vec![3])?;
    assert_eq!(
        branch.scan_prefix(b"p/")?,
        vec![(b"p/b".to_vec(), vec![2]), (b"p/c".to_vec(), vec![3])]
    );
    assert!(
        branch
            .freeze()?
            .dependencies()
            .iter()
            .any(|dependency| matches!(dependency, Dependency::Namespace { .. }))
    );
    Ok(())
}
