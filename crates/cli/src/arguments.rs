use std::ffi::OsString;
use std::path::PathBuf;

pub(super) const HELP: &str = "NDDev Clipboard native client

nddev-clipboard [--config ABSOLUTE_FILE] [--timeout-ms N] COMMAND

list [-n N|--limit N] [--before ID] [-q TEXT|--query TEXT]
     [-k KIND|--kind KIND] [--favorites|--unpinned]
get ID [-m MIME|--mime MIME]   Stream the representation to standard output
pin ID | unpin ID            Change a favorite
remove ID                    Remove one entry, including a favorite
clear                        Clear unpinned history; preserve favorites
stats                        Read archive counters and retention policy
watch --seconds N            Observe events for an explicit finite lifetime

Control output is one escaped JSON value per line; get writes raw bytes.
Watch first emits the v1 hello, then archive notifications.
Normal operations have a finite provider deadline. Watch owns its deadline
and cannot be combined with --timeout-ms. Output backpressure belongs to
the receiving process. No command starts capture or opens the archive.
--help and --version read no profile and open no connection.";

pub(super) struct Arguments {
    pub(super) config: Option<PathBuf>,
    pub(super) timeout_ms: u64,
    pub(super) command: Command,
}

#[derive(Default)]
pub(super) struct Listing {
    pub(super) limit: Option<u32>,
    pub(super) before: Option<i64>,
    pub(super) query: Option<String>,
    pub(super) kind: Option<String>,
    pub(super) pinned: Option<bool>,
}

pub(super) enum Command {
    Help,
    Version,
    List(Listing),
    Get { entry: i64, mime: Option<String> },
    Pin { entry: i64, pinned: bool },
    Remove { entry: i64 },
    Clear,
    Stats,
    Watch { seconds: u64 },
}

impl Arguments {
    pub(super) fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Self, &'static str> {
        let mut arguments = arguments.peekable();
        let mut config = None;
        let mut timeout_ms = None;
        while let Some(option) = arguments.peek().and_then(|value| value.to_str()) {
            match option {
                "--config" if config.is_none() => {
                    arguments.next();
                    config = Some(PathBuf::from(next(&mut arguments)?));
                }
                "--timeout-ms" if timeout_ms.is_none() => {
                    arguments.next();
                    timeout_ms = Some(number(&mut arguments)?);
                }
                _ => break,
            }
        }
        let command = text(&mut arguments)?;
        let command = match command.as_str() {
            "--help" | "-h" => Command::Help,
            "--version" | "-V" => Command::Version,
            "list" => {
                let mut listing = Listing::default();
                while arguments.peek().is_some() {
                    match text(&mut arguments)?.as_str() {
                        "--limit" | "-n" if listing.limit.is_none() => {
                            listing.limit = Some(
                                u32::try_from(number(&mut arguments)?)
                                    .map_err(|_| "limit is too large")?,
                            );
                        }
                        "--before" if listing.before.is_none() => {
                            listing.before = Some(entry(&mut arguments)?);
                        }
                        "--query" | "-q" if listing.query.is_none() => {
                            listing.query = Some(text(&mut arguments)?);
                        }
                        "--kind" | "-k" if listing.kind.is_none() => {
                            listing.kind = Some(text(&mut arguments)?);
                        }
                        "--favorites" if listing.pinned.is_none() => listing.pinned = Some(true),
                        "--unpinned" if listing.pinned.is_none() => listing.pinned = Some(false),
                        _ => return Err("unknown, duplicate or conflicting list option"),
                    }
                }
                Command::List(listing)
            }
            "get" => {
                let entry = entry(&mut arguments)?;
                let mime = match arguments.next() {
                    None => None,
                    Some(option) if matches!(option.to_str(), Some("--mime" | "-m")) => {
                        Some(text(&mut arguments)?)
                    }
                    _ => return Err("unknown get option"),
                };
                Command::Get { entry, mime }
            }
            "pin" | "unpin" => Command::Pin {
                entry: entry(&mut arguments)?,
                pinned: command == "pin",
            },
            "remove" => Command::Remove {
                entry: entry(&mut arguments)?,
            },
            "clear" => Command::Clear,
            "stats" => Command::Stats,
            "watch" if timeout_ms.is_none() => {
                if text(&mut arguments)? != "--seconds" {
                    return Err("watch requires --seconds N");
                }
                Command::Watch {
                    seconds: number(&mut arguments)?,
                }
            }
            _ => return Err("unknown command or conflicting deadline; use --help"),
        };
        if arguments.next().is_some() {
            return Err("unexpected command-line argument");
        }
        Ok(Self {
            config,
            timeout_ms: timeout_ms.unwrap_or(10_000),
            command,
        })
    }
}

fn next(arguments: &mut impl Iterator<Item = OsString>) -> Result<OsString, &'static str> {
    arguments.next().ok_or("command-line value is missing")
}

fn text(arguments: &mut impl Iterator<Item = OsString>) -> Result<String, &'static str> {
    next(arguments)?
        .into_string()
        .map_err(|_| "command-line text is invalid")
}

fn number(arguments: &mut impl Iterator<Item = OsString>) -> Result<u64, &'static str> {
    text(arguments)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or("expected a positive integer")
}

fn entry(arguments: &mut impl Iterator<Item = OsString>) -> Result<i64, &'static str> {
    i64::try_from(number(arguments)?).map_err(|_| "entry identifier is too large")
}
