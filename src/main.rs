mod cli;
mod engine;
mod error;
mod platform;

use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};

use crate::{cli::Cli, error::MkaError};

fn main() -> ExitCode {
    let cli = Cli::parse();

    match engine::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(MkaError::Usage(message)) => {
            Cli::command()
                .error(ErrorKind::ValueValidation, message)
                .exit();
        }
        Err(MkaError::Runtime(message)) => {
            eprintln!("mka: {message}");
            ExitCode::FAILURE
        }
    }
}
