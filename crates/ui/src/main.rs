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
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() {
        paint_ui::native::run()?;
        return Ok(());
    }
    let report = paint_ui::headless::run_cli(arguments)?;
    writeln!(io::stdout().lock(), "{report}")?;
    Ok(())
}
