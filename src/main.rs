mod cli;

use kk::Error;
use std::{
    io::{self, Write},
    process::ExitCode,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let result = cli::run().await;
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
