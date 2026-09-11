mod cli;
mod engine;
mod error;
mod platform;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};

use crate::{
    cli::Cli,
    error::{MkaError, Result},
};

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
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

fn run(cli: Cli) -> Result<()> {
    if let Some(shell) = cli.completion {
        let mut script = Vec::new();
        clap_complete::generate(shell, &mut Cli::command(), "mka", &mut script);
        return match io::stdout().lock().write_all(&script) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(error) => Err(MkaError::runtime(format!(
                "cannot write completion script: {error}"
            ))),
        };
    }

    engine::run(cli)
}
