#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mutation {
    Put(Vec<u8>),
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dependency {
    Key { key: Vec<u8>, version: Version },
    Namespace { epoch: u64 },
}
