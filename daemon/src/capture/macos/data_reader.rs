//! Immutable Foundation data read through safe range copies. The caller's
//! streaming buffer bounds each temporary allocation; no full Rust copy is made.
#![forbid(unsafe_code)]

use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{NSCopying, NSData, NSRange};
use std::io::{self, Read};

pub(super) struct DataReader {
    data: Retained<NSData>,
    offset: usize,
}

impl DataReader {
    pub(super) fn new(data: &NSData) -> Self {
        // NSCopying returns an immutable snapshot, including when the dynamic
        // source is mutable. Already immutable NSData can share its storage.
        Self {
            data: data.copy(),
            offset: 0,
        }
    }

    pub(super) fn content_length(&self) -> usize {
        self.data.len()
    }
}

impl Read for DataReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min(self.data.len() - self.offset);
        if count == 0 {
            return Ok(0);
        }
        // Drain Foundation's autoreleased range object on every read, rather
        // than retaining one object per chunk until a large transfer finishes.
        let bytes = autoreleasepool(|_| {
            self.data
                .subdataWithRange(NSRange::new(self.offset, count))
                .to_vec()
        });
        if bytes.len() != count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native data range was incomplete",
            ));
        }
        output[..count].copy_from_slice(&bytes);
        self.offset += count;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSMutableData;

    #[test]
    fn binary_data_survives_uneven_reads_and_eof_without_unchecked_borrows() {
        let expected: Vec<u8> = (0..=255).cycle().take(3 * 1024 * 1024 + 37).collect();
        let data = NSData::with_bytes(&expected);
        let mut reader = DataReader::new(&data);
        assert_eq!(reader.content_length(), expected.len());
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        let mut actual = Vec::new();
        let mut buffer = [0; 997];
        loop {
            let count = reader.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            actual.extend_from_slice(&buffer[..count]);
        }
        assert_eq!(actual, expected);
        assert_eq!(reader.read(&mut buffer).unwrap(), 0);
    }

    #[test]
    fn an_immutable_snapshot_survives_changes_to_the_original_mutable_object() {
        let original = NSMutableData::with_bytes(b"captured bytes");
        let mut reader = DataReader::new(&original);
        original.set_bytes(b"a later and differently sized native value");
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, b"captured bytes");
    }
}
