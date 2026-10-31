mod cli;

use kk::Error;
use std::{
    io::{self, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    #[cfg(any(unix, windows))]
    if let Some(status) = kk::stdio_worker::dispatch() {
        return status;
    }
    let result = run();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if writeln!(io::stderr(), "{}: {error}", error.code()).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::from(if matches!(error, Error::Cancelled) {
                130
            } else {
                1
            })
        }
    }
}

fn run() -> kk::Result<()> {
    #[cfg(any(target_vendor = "apple", windows))]
    kk::stdio_worker::configure(std::env::current_exe()?)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(cli::run())
}
