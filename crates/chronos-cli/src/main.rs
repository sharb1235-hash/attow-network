//! Attow Network development command-line interface.
use std::{error::Error, io};
const HELP: &str = "Attow Network 0.1.0 — development workspace\nUsage: attow [--help | --version]\nRuntime commands are not implemented yet.";
fn run(args: &[String]) -> Result<String, io::Error> {
    match args {
        [] => Ok(HELP.to_owned()),
        [arg] if arg == "--help" || arg == "-h" => Ok(HELP.to_owned()),
        [arg] if arg == "--version" || arg == "-V" => {
            Ok("Attow Network 0.1.0 (development)".to_owned())
        }
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported command: runtime commands are not implemented yet",
        )),
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    println!("{}", run(&std::env::args().skip(1).collect::<Vec<_>>())?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_reports_development_status() -> Result<(), io::Error> {
        let help = run(&["--help".to_owned()])?;
        assert!(help.contains("Attow Network"));
        assert!(help.contains("not implemented"));
        Ok(())
    }
    #[test]
    fn unsupported_command_fails() {
        assert!(run(&["promote".to_owned()]).is_err());
    }
}
