use std::{error::Error, path::Path};
fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no repository parent")?;
    xtask::commands::run(root, std::env::args_os().skip(1))
}
