use crate::{BlobInfo, Client, Error};
use std::io::{self, Read, Write};

/// A bounded representation stream that exclusively borrows its connection.
///
/// Reading the declared length allows later requests. A failed or incomplete
/// stream closes the connection instead of exposing leftover bytes as a frame.
pub struct BlobReader<'a, T: Read + Write> {
    pub(crate) client: &'a mut Client<T>,
    info: BlobInfo,
    remaining: u64,
    failed: bool,
}

impl<'a, T: Read + Write> BlobReader<'a, T> {
    pub(crate) fn new(client: &'a mut Client<T>, info: BlobInfo) -> Self {
        Self {
            client,
            remaining: info.bytes,
            info,
            failed: false,
        }
    }

    pub fn info(&self) -> &BlobInfo {
        &self.info
    }

    /// Confirm complete consumption and return the representation metadata.
    pub fn finish(mut self) -> Result<BlobInfo, Error> {
        if self.failed {
            return Err(Error::Closed);
        }
        if self.remaining != 0 {
            self.failed = true;
            return self.client.invalidate(Error::TruncatedPayload);
        }
        self.client.check()?;
        Ok(self.info.clone())
    }

    fn fail(&mut self, kind: io::ErrorKind) -> io::Error {
        self.failed = true;
        self.client.close();
        kind.into()
    }
}

impl<T: Read + Write> Read for BlobReader<'_, T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.client.io_check()?;
        if bytes.is_empty() || self.remaining == 0 {
            return Ok(0);
        }
        let limit = self.remaining.min(bytes.len() as u64) as usize;
        // The control-frame reader may already hold payload or later frames.
        let count = match self.client.stream.read(&mut bytes[..limit]) {
            Ok(0) => return Err(self.fail(io::ErrorKind::UnexpectedEof)),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
            Err(error) => return Err(self.fail(error.kind())),
        };
        self.client.io_check()?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

impl<T: Read + Write> Drop for BlobReader<'_, T> {
    fn drop(&mut self) {
        if self.failed || self.remaining != 0 {
            self.client.close();
        }
    }
}

/// A declared representation whose acknowledgement is owned by this writer.
///
/// Dropping it without `finish` invalidates the connection, including when all
/// bytes were written: the pending acknowledgement must not become another reply.
pub struct PartWriter<'a, T: Read + Write> {
    pub(crate) client: &'a mut Client<T>,
    request: u64,
    remaining: u64,
    acknowledged: bool,
}

impl<'a, T: Read + Write> PartWriter<'a, T> {
    pub(crate) fn new(client: &'a mut Client<T>, request: u64, bytes: u64) -> Self {
        Self {
            client,
            request,
            remaining: bytes,
            acknowledged: false,
        }
    }

    /// Flush a complete representation and consume its matching response.
    pub fn finish(mut self) -> Result<(), Error> {
        self.client.check()?;
        if self.remaining != 0 {
            return self.client.invalidate(Error::TruncatedPayload);
        }
        if let Err(error) = self.flush() {
            return self.client.invalidate(Error::Transport(error.kind()));
        }
        let result = self.client.expect_ok(self.request);
        // A matching refusal also consumes the response and permits reuse.
        // Invalid frames and transport failures already close the client.
        self.acknowledged = true;
        result
    }

    fn fail(&mut self, kind: io::ErrorKind) -> io::Error {
        self.client.close();
        kind.into()
    }
}

impl<T: Read + Write> Write for PartWriter<'_, T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.client.io_check()?;
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(self.fail(io::ErrorKind::WriteZero));
        }
        let limit = self.remaining.min(bytes.len() as u64) as usize;
        let count = match self.client.stream.get_mut().write(&bytes[..limit]) {
            Ok(0) => return Err(self.fail(io::ErrorKind::WriteZero)),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
            Err(error) => return Err(self.fail(error.kind())),
        };
        self.client.io_check()?;
        self.remaining -= count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.client.io_check()?;
        if let Err(error) = self.client.stream.get_mut().flush() {
            return Err(self.fail(error.kind()));
        }
        self.client.io_check()
    }
}

impl<T: Read + Write> Drop for PartWriter<'_, T> {
    fn drop(&mut self) {
        if !self.acknowledged {
            self.client.close();
        }
    }
}
