//! Streaming client for the existing local protocol, with no archive access.
//!
//! Callers can supply an owned transport, or use [`LocalOperation`] on native
//! platforms for a pathname socket with one absolute deadline and cancellation.
//! This crate never starts a collector.
//! Control frames are bounded; representation bytes use fixed-size buffers.
//! Interrupted or malformed exchanges invalidate the connection so leftover
//! payload bytes can never be interpreted as another response.
use nddev_clipboard_api::wire::{
    ArchiveStats, MAX_FRAME, PROTOCOL_VERSION, Request, Response, Role, Summary,
};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::Arc;

#[cfg(any(unix, windows))]
mod local;
mod payload;
mod policy;
mod subscription;
#[cfg(any(unix, windows))]
pub use local::{Cancellation, LocalOperation, LocalTransport};
pub use payload::{BlobReader, PartWriter};
pub use subscription::Subscription;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Clipboard transport failed ({0:?})")]
    Transport(io::ErrorKind),
    #[error("Clipboard control frame is invalid or too large")]
    InvalidFrame,
    #[error("Clipboard protocol version does not match")]
    Version,
    #[error("Clipboard response does not match the request")]
    UnexpectedResponse,
    #[error("Clipboard representation ended early")]
    TruncatedPayload,
    #[error("Clipboard connection cannot be reused")]
    Closed,
    #[error("Clipboard request was refused")]
    Remote { code: String },
}

pub struct EntryPage {
    pub items: Vec<Summary>,
    pub more: bool,
}

/// Omitted fields use the provider's browsing policy. Text stays borrowed until
/// the bounded request is encoded; the SDK never opens the archive to filter it.
#[derive(Default)]
pub struct ListQuery<'a> {
    pub limit: Option<u32>,
    pub before: Option<i64>,
    pub query: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub pinned: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct BlobInfo {
    pub mime: String,
    pub bytes: u64,
}

pub struct Client<T> {
    stream: BufReader<T>,
    next_request: u64,
    usable: bool,
    policy: Option<Arc<policy::Policy>>,
}

impl<T: Read + Write> Client<T> {
    /// Negotiate v1 without asynchronous broadcasts. One owner serializes calls
    /// on this connection; other clients may still update the same archive.
    pub fn new(transport: T, role: Role) -> Result<Self, Error> {
        Self::with_policy(transport, role, None)
    }

    fn with_policy(
        transport: T,
        role: Role,
        policy: Option<Arc<policy::Policy>>,
    ) -> Result<Self, Error> {
        Self::negotiate(transport, role, false, policy).map(|(client, _)| client)
    }

    fn negotiate(
        transport: T,
        role: Role,
        watch: bool,
        policy: Option<Arc<policy::Policy>>,
    ) -> Result<(Self, Response), Error> {
        let mut client = Self {
            stream: BufReader::with_capacity(8192, transport),
            next_request: 1,
            usable: true,
            policy,
        };
        client.send(&Request::Hello {
            v: PROTOCOL_VERSION,
            role,
            watch,
        })?;
        match client.frame()? {
            greeting @ Response::Hello { v, .. } if v == PROTOCOL_VERSION => Ok((client, greeting)),
            Response::Hello { .. } => Err(Error::Version),
            Response::Error { code, .. } => Err(Error::Remote { code }),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub fn list(&mut self, query: ListQuery<'_>) -> Result<EntryPage, Error> {
        control_text(query.query)?;
        control_text(query.kind)?;
        let req = self.request_id()?;
        self.send(&Request::List {
            req,
            limit: query.limit,
            before: query.before,
            query: query.query.map(str::to_owned),
            kind: query.kind.map(str::to_owned),
            pinned: query.pinned,
        })?;
        match self.answer(req)? {
            Response::List { items, more, .. } => Ok(EntryPage { items, more }),
            _ => self.invalidate(Error::UnexpectedResponse),
        }
    }

    pub fn pin(&mut self, entry: i64, pinned: bool) -> Result<(), Error> {
        let req = self.request_id()?;
        self.send(&Request::Pin { req, entry, pinned })?;
        self.expect_ok(req)
    }

    pub fn remove(&mut self, entry: i64) -> Result<(), Error> {
        let req = self.request_id()?;
        self.send(&Request::Remove { req, entry })?;
        self.expect_ok(req)
    }

    /// Remove unpinned history. Favorites retain the provider's durable policy.
    pub fn clear(&mut self) -> Result<(), Error> {
        let req = self.request_id()?;
        self.send(&Request::Clear { req })?;
        self.expect_ok(req)
    }

    pub fn stats(&mut self) -> Result<ArchiveStats, Error> {
        let req = self.request_id()?;
        self.send(&Request::Stats { req })?;
        match self.answer(req)? {
            Response::Stats { stats, .. } => Ok(stats),
            _ => self.invalidate(Error::UnexpectedResponse),
        }
    }

    pub fn begin(&mut self, source: Option<&str>) -> Result<u64, Error> {
        control_text(source)?;
        let req = self.request_id()?;
        self.send(&Request::Begin {
            req,
            source: source.map(str::to_owned),
        })?;
        match self.answer(req)? {
            Response::Begin { draft, .. } => Ok(draft),
            _ => self.invalidate(Error::UnexpectedResponse),
        }
    }

    /// Send exactly the declared length. No complete payload allocation occurs.
    /// On source failure, drop this client; the daemon discards its open drafts.
    pub fn part_from(
        &mut self,
        draft: u64,
        mime: &str,
        bytes: u64,
        input: &mut impl Read,
    ) -> Result<(), Error> {
        let policy = self.policy.clone();
        let mut writer = self.part_writer(draft, mime, bytes)?;
        match policy::copy(input, &mut writer, bytes, policy.as_deref()) {
            Ok(count) if count == bytes => writer.finish(),
            Ok(_) => writer.client.invalidate(Error::TruncatedPayload),
            Err(error) => writer.client.invalidate(Error::Transport(error.kind())),
        }
    }

    /// Borrow the connection while an encoder writes one declared representation.
    /// `finish` is required to consume the acknowledgement before another request.
    pub fn part_writer(
        &mut self,
        draft: u64,
        mime: &str,
        bytes: u64,
    ) -> Result<PartWriter<'_, T>, Error> {
        control_text(Some(mime))?;
        if bytes > i64::MAX as u64 {
            return Err(Error::InvalidFrame);
        }
        let req = self.request_id()?;
        self.send(&Request::Part {
            req,
            draft,
            mime: mime.to_owned(),
            bytes: Some(bytes),
        })?;
        Ok(PartWriter::new(self, req, bytes))
    }

    pub fn commit(&mut self, draft: u64) -> Result<i64, Error> {
        let req = self.request_id()?;
        self.send(&Request::Commit { req, draft })?;
        match self.answer(req)? {
            Response::Commit { entry, .. } => Ok(entry),
            _ => self.invalidate(Error::UnexpectedResponse),
        }
    }

    pub fn abort(&mut self, draft: u64) -> Result<(), Error> {
        let req = self.request_id()?;
        self.send(&Request::Abort { req, draft })?;
        self.expect_ok(req)
    }

    pub fn fetch_to(
        &mut self,
        entry: i64,
        mime: Option<&str>,
        transcode: bool,
        output: &mut impl Write,
    ) -> Result<BlobInfo, Error> {
        let policy = self.policy.clone();
        let mut reader = self.fetch_reader(entry, mime, transcode)?;
        let bytes = reader.info().bytes;
        match policy::copy(&mut reader, output, bytes, policy.as_deref()) {
            Ok(count) if count == bytes => reader.finish(),
            Ok(_) => reader.client.invalidate(Error::TruncatedPayload),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                reader.client.invalidate(Error::TruncatedPayload)
            }
            Err(error) => reader.client.invalidate(Error::Transport(error.kind())),
        }
    }

    /// Borrow the connection while a decoder consumes one representation.
    /// Dropping an incomplete reader prevents reuse of the connection.
    pub fn fetch_reader(
        &mut self,
        entry: i64,
        mime: Option<&str>,
        transcode: bool,
    ) -> Result<BlobReader<'_, T>, Error> {
        control_text(mime)?;
        let req = self.request_id()?;
        self.send(&Request::Fetch {
            req,
            entry,
            mime: mime.map(str::to_owned),
            transcode,
        })?;
        let Response::Blob { mime, bytes, .. } = self.answer(req)? else {
            return self.invalidate(Error::UnexpectedResponse);
        };
        if bytes > i64::MAX as u64 {
            return self.invalidate(Error::InvalidFrame);
        }
        Ok(BlobReader::new(self, BlobInfo { mime, bytes }))
    }

    fn request_id(&mut self) -> Result<u64, Error> {
        self.check()?;
        let req = self.next_request;
        self.next_request = req.checked_add(1).ok_or(Error::Closed)?;
        Ok(req)
    }

    fn send(&mut self, request: &Request) -> Result<(), Error> {
        self.check()?;
        let mut frame = Frame(Vec::new());
        serde_json::to_writer(&mut frame, request).map_err(|_| Error::InvalidFrame)?;
        frame.0.push(b'\n');
        if let Err(error) = self
            .stream
            .get_mut()
            .write_all(&frame.0)
            .and_then(|()| self.stream.get_mut().flush())
        {
            return self.invalidate(Error::Transport(error.kind()));
        }
        Ok(())
    }

    fn frame(&mut self) -> Result<Response, Error> {
        self.check()?;
        let mut line = Vec::new();
        match self
            .stream
            .by_ref()
            .take(MAX_FRAME as u64 + 1)
            .read_until(b'\n', &mut line)
        {
            Ok(length) if length <= MAX_FRAME && line.last() == Some(&b'\n') => {
                self.check()?;
                match serde_json::from_slice(&line) {
                    Ok(frame) => Ok(frame),
                    Err(_) => self.invalidate(Error::InvalidFrame),
                }
            }
            Ok(_) => self.invalidate(Error::InvalidFrame),
            Err(error) => self.invalidate(Error::Transport(error.kind())),
        }
    }

    fn answer(&mut self, expected: u64) -> Result<Response, Error> {
        let response = self.frame()?;
        let req = match &response {
            Response::Ok { req }
            | Response::Begin { req, .. }
            | Response::Commit { req, .. }
            | Response::List { req, .. }
            | Response::Blob { req, .. }
            | Response::Thumb { req, .. }
            | Response::Stats { req, .. } => Some(*req),
            Response::Error { req, .. } => *req,
            _ => None,
        };
        if req != Some(expected) {
            return self.invalidate(Error::UnexpectedResponse);
        }
        match response {
            Response::Error { code, .. } => Err(Error::Remote { code }),
            response => Ok(response),
        }
    }

    fn expect_ok(&mut self, req: u64) -> Result<(), Error> {
        match self.answer(req)? {
            Response::Ok { .. } => Ok(()),
            _ => self.invalidate(Error::UnexpectedResponse),
        }
    }

    fn invalidate<R>(&mut self, error: Error) -> Result<R, Error> {
        self.close();
        Err(error)
    }

    fn close(&mut self) {
        self.usable = false;
        if let Some(policy) = &self.policy {
            policy.shutdown();
        }
    }

    fn check(&mut self) -> Result<(), Error> {
        if !self.usable {
            return Err(Error::Closed);
        }
        if let Some(policy) = &self.policy
            && let Err(error) = policy.remaining()
        {
            return self.invalidate(Error::Transport(error.kind()));
        }
        Ok(())
    }

    fn io_check(&mut self) -> io::Result<()> {
        self.check().map_err(|error| match error {
            Error::Transport(kind) => kind.into(),
            _ => io::ErrorKind::NotConnected.into(),
        })
    }
}

fn control_text(value: Option<&str>) -> Result<(), Error> {
    if value.is_some_and(|value| value.len() >= MAX_FRAME) {
        return Err(Error::InvalidFrame);
    }
    Ok(())
}

struct Frame(Vec<u8>);
impl Write for Frame {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() >= MAX_FRAME - self.0.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
