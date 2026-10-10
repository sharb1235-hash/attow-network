use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next()) {
        (Some("verify-repository"), None) => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .ok_or("xtask has no repository parent")?;
            xtask::verify_repository(root)?;
            println!("Attow Network repository checks passed.");
            Ok(())
        }
        (Some("--help" | "-h"), None) | (None, None) => {
            println!(
                "Attow Network development repository tools\n\nUsage: cargo run -p xtask -- verify-repository\n\nOnly repository foundation checks are implemented."
            );
            Ok(())
        }
        _ => Err("unsupported xtask command; use --help for implemented commands".into()),
    }
}
