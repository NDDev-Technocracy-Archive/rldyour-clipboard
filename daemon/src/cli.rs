use nddev_clipboard_api::{Config, default_config_path};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Serve,
    Socket,
    Check,
    Initialize,
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let mut operation = Operation::Serve;
    let mut path = None;
    let mut initialization = crate::profile::Initialization::default();
    while let Some(argument) = arguments.next() {
        let text = argument.to_str().ok_or("invalid command-line option")?;
        match text {
            "--version" | "-V" => {
                println!("rldyour-clipboardd {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "NDDev Clipboard {}\n\n\
                    rldyour-clipboardd [--config ABSOLUTE_FILE]\n\
                    rldyour-clipboardd --socket-json [--config ABSOLUTE_FILE]\n\
                    rldyour-clipboardd --check-config [--config ABSOLUTE_FILE]\n\
                    rldyour-clipboardd --init-config [--config ABSOLUTE_FILE]\n\
                    \x20 [--archive ABSOLUTE_DIR] [--socket ABSOLUTE_PATH]\n\
                    \x20 [--budget-bytes N] [--retention-days N] [--capture true|false]\n\n\
                    Runtime requires a complete TOML file. --socket-json reads settings\n\
                    only; it opens no archive or connection and starts no collector.\n\
                    Initialization preserves existing files and archive placement.\n\
                    Existing archives require explicit budget, retention and capture values.",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            "--config" if path.is_none() => path = Some(PathBuf::from(next(&mut arguments)?)),
            "--socket-json" | "--check-config" | "--init-config"
                if operation == Operation::Serve =>
            {
                operation = match text {
                    "--socket-json" => Operation::Socket,
                    "--check-config" => Operation::Check,
                    _ => Operation::Initialize,
                };
            }
            "--archive" if initialization.archive.is_none() => {
                initialization.archive = Some(PathBuf::from(next(&mut arguments)?))
            }
            "--socket" if initialization.socket.is_none() => {
                initialization.socket = Some(PathBuf::from(next(&mut arguments)?))
            }
            "--budget-bytes" if initialization.budget.is_none() => {
                initialization.budget = Some(number(next(&mut arguments)?)?)
            }
            "--retention-days" if initialization.retention.is_none() => {
                initialization.retention = Some(u32::try_from(number(next(&mut arguments)?)?)?)
            }
            "--capture" if initialization.capture.is_none() => {
                initialization.capture = Some(match next(&mut arguments)?.to_str() {
                    Some("true") => true,
                    Some("false") => false,
                    _ => return Err("--capture expects true or false".into()),
                })
            }
            _ => return Err("unknown, duplicate or conflicting command-line option".into()),
        }
    }
    if !initialization.is_empty() && operation != Operation::Initialize {
        return Err("profile values are accepted only with --init-config".into());
    }
    let path = path.map(Ok).unwrap_or_else(default_config_path)?;
    if operation == Operation::Initialize {
        crate::profile::initialize(&path, initialization)?;
        println!("configuration ready");
        return Ok(());
    }
    let config = Config::read(&path)?;
    match operation {
        Operation::Serve => crate::server::run(config).map_err(Into::into),
        Operation::Socket => {
            serde_json::to_writer(std::io::stdout().lock(), config.socket_path())?;
            println!();
            Ok(())
        }
        Operation::Check => {
            println!("configuration valid");
            Ok(())
        }
        Operation::Initialize => unreachable!(),
    }
}

fn next(
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<OsString, Box<dyn std::error::Error>> {
    arguments
        .next()
        .ok_or_else(|| "command-line value is missing".into())
}

fn number(value: OsString) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(value.to_str().ok_or("numeric value is invalid")?.parse()?)
}
