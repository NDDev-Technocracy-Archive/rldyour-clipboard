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

    fn check(&mut self) -> io::Result<()> {
        self.client.check().map_err(|error| match error {
            Error::Transport(kind) => kind.into(),
            _ => io::ErrorKind::NotConnected.into(),
        })
    }

    fn fail(&mut self, kind: io::ErrorKind) -> io::Error {
        self.failed = true;
        self.client.close();
        kind.into()
    }
}

impl<T: Read + Write> Read for BlobReader<'_, T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
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
        self.check()?;
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
