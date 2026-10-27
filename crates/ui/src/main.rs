use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("paint-ui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let report = paint_ui::headless::run_cli(std::env::args_os().skip(1))?;
    writeln!(io::stdout().lock(), "{report}")?;
    Ok(())
}
