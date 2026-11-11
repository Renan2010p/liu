// SPDX-License-Identifier: MPL-2.0

//! `liu-kernel`: the kernel process.
//!
//! Exit status: 0 when the kernel returned `STATUS_OK`, 1 when the runtime
//! failed, 2 when the kernel returned an error, 3 after a kernel panic.

#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;

use liu_abi::{Error, Status};
use liu_runtime::{Config, RuntimeError};

fn
main() -> ExitCode
{
    let config: Config = match Config::parse(env::args().skip(1))
    {
        Ok(config) => config,
        Err(message) =>
        {
            eprintln!("liu-kernel: {}\n{}", message, liu_runtime::config::USAGE);
            return (ExitCode::from(1));
        }
    };
    let result: Result<Status, RuntimeError> = liu_runtime::run(&config);

    match result
    {
        Ok(status) =>
        {
            if let Err(error) = Error::check(status)
            {
                eprintln!("liu-kernel: kernel returned an error: {}", error);
                return (ExitCode::from(2));
            }
        }
        Err(error) =>
        {
            eprintln!("liu-kernel: {}", error);
            return (ExitCode::from(1));
        }
    }

    return (ExitCode::SUCCESS);
}
