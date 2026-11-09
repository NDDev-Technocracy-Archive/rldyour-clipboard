//! A local client only. Archive access and native capture remain in the daemon.
use nddev_clipboard_api::desktop::{DesktopEntry, DesktopResult};
use nddev_clipboard_api::wire::Role;
use nddev_clipboard_api::{Config, default_config_path};
use nddev_clipboard_sdk::{Error, ListQuery, LocalOperation};
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod arguments;
use arguments::{Arguments, Command};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nddev-clipboard: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    // Parse every option before resolving a profile or opening a connection.
    let arguments = Arguments::parse(std::env::args_os().skip(1))?;
    match arguments.command {
        Command::Help => {
            println!("{}", arguments::HELP);
            return Ok(());
        }
        Command::Version => {
            println!("nddev-clipboard {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }
    let path = arguments
        .config
        .map(Ok)
        .unwrap_or_else(default_config_path)?;
    let config = Config::read(&path)?;
    let started = Instant::now();
    let duration = match arguments.command {
        Command::Watch { seconds } => Duration::from_secs(seconds),
        _ => Duration::from_millis(arguments.timeout_ms),
    };
    let deadline = started
        .checked_add(duration)
        .ok_or("deadline is too large")?;
    let operation = LocalOperation::new(deadline);
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if matches!(arguments.command, Command::Watch { .. }) {
        let mut subscription = operation.subscribe(config.socket_path())?;
        json(&mut output, subscription.greeting())?;
        loop {
            match subscription.next_event() {
                Ok(event) => json(&mut output, &event)?,
                Err(Error::Transport(io::ErrorKind::TimedOut)) if Instant::now() >= deadline => {
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    let mut client = operation.connect(config.socket_path(), Role::Ui)?;
    match arguments.command {
        Command::List(options) => {
            let page = client.list(ListQuery {
                limit: options.limit,
                before: options.before,
                query: options.query.as_deref(),
                kind: options.kind.as_deref(),
                pinned: options.pinned,
            })?;
            json(
                &mut output,
                &DesktopResult::Page {
                    items: page.items.into_iter().map(DesktopEntry::from).collect(),
                    more: page.more,
                },
            )?;
        }
        Command::Get { entry, mime } => {
            client.fetch_to(entry, mime.as_deref(), false, &mut output)?;
            output.flush()?;
        }
        Command::Pin { entry, pinned } => {
            client.pin(entry, pinned)?;
            json(&mut output, &DesktopResult::Changed {})?;
        }
        Command::Remove { entry } => {
            client.remove(entry)?;
            json(&mut output, &DesktopResult::Changed {})?;
        }
        Command::Clear => {
            client.clear()?;
            json(&mut output, &DesktopResult::Changed {})?;
        }
        Command::Stats => json(&mut output, &client.stats()?)?,
        Command::Help | Command::Version | Command::Watch { .. } => unreachable!(),
    }
    Ok(())
}

fn json(output: &mut impl Write, value: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
