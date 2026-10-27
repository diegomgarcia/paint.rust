use std::process::ExitCode;

fn main() -> ExitCode {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    ExitCode::from(xtask::cli::run(
        std::env::args_os().skip(1),
        &mut out,
        &mut err,
    ))
}
