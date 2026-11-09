//! Adversarial peers exercise framing and connection reuse, never an OS clipboard.
use nddev_clipboard_api::wire::{MAX_FRAME, Role};
use nddev_clipboard_sdk::{Client, Error, ListQuery, Subscription};
use std::io::{self, Cursor, Read, Write};

struct Peer {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}
impl Read for Peer {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.input.read(bytes)
    }
}
impl Write for Peer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.output.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn peer(responses: &[u8]) -> Peer {
    let mut input =
        br#"{"ev":"hello","v":1,"entries":0,"bytes":0,"budget":1024,"retention_days":null}"#
            .to_vec();
    input.push(b'\n');
    input.extend_from_slice(responses);
    Peer {
        input: Cursor::new(input),
        output: Vec::new(),
    }
}

fn client(responses: &[u8]) -> Client<Peer> {
    Client::new(peer(responses), Role::Both).unwrap()
}

#[test]
fn notification_ownership_rejects_a_request_reply_and_prevents_reuse() {
    let mut subscription = Subscription::new(peer(b"{\"ev\":\"ok\",\"req\":1}\n")).unwrap();
    assert!(matches!(
        subscription.next_event(),
        Err(Error::UnexpectedResponse)
    ));
    assert!(matches!(subscription.next_event(), Err(Error::Closed)));
}

#[test]
fn malformed_thumbnail_geometry_cannot_write_pixels_or_reuse_the_connection() {
    let mut client = client(b"{\"ev\":\"thumb\",\"req\":1,\"width\":4294967295,\"height\":1,\"stride\":4,\"bytes\":4}\n1234");
    let mut pixels = Vec::new();
    assert!(matches!(
        client.thumbnail_to(1, &mut pixels),
        Err(Error::InvalidFrame)
    ));
    assert!(pixels.is_empty());
    assert!(matches!(client.pin(1, true), Err(Error::Closed)));
}

#[test]
fn thumbnail_pixels_stop_before_a_prefetched_control_frame() {
    let mut responses =
        b"{\"ev\":\"thumb\",\"req\":1,\"width\":1,\"height\":1,\"stride\":4,\"bytes\":4}\n"
            .to_vec();
    responses.extend_from_slice(&[0xff, b'\n', 0, 3]);
    responses.extend_from_slice(b"{\"ev\":\"ok\",\"req\":2}\n");
    let mut client = client(&responses);
    let mut pixels = Vec::new();
    assert_eq!(client.thumbnail_to(1, &mut pixels).unwrap().bytes, 4);
    assert_eq!(pixels, [0xff, b'\n', 0, 3]);
    client.pin(1, true).unwrap();
}

#[test]
fn prefetched_binary_and_the_next_control_frame_keep_their_boundaries() {
    let mut responses =
        br#"{"ev":"blob","req":1,"mime":"application/octet-stream","bytes":4}"#.to_vec();
    responses.extend_from_slice(&[b'\n', 0xff, b'\n', 0, 3]);
    responses.extend_from_slice(b"{\"ev\":\"ok\",\"req\":2}\n");
    let mut client = client(&responses);
    let mut reader = client.fetch_reader(i64::MAX, None, false).unwrap();
    let mut payload = [0; 4];
    reader.read_exact(&mut payload).unwrap();
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    let blob = reader.finish().unwrap();
    assert_eq!(payload, [0xff, b'\n', 0, 3]);
    assert_eq!(blob.mime, "application/octet-stream");
    assert_eq!(blob.bytes, 4);
    client.pin(i64::MAX, true).unwrap();
}

#[test]
fn an_empty_representation_does_not_consume_the_following_frame() {
    let mut client = client(b"{\"ev\":\"blob\",\"req\":1,\"mime\":\"text/plain\",\"bytes\":0}\n{\"ev\":\"ok\",\"req\":2}\n");
    let mut reader = client.fetch_reader(1, None, false).unwrap();
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    assert_eq!(reader.finish().unwrap().bytes, 0);
    client.pin(1, true).unwrap();
}

#[test]
fn an_upload_consumes_its_acknowledgement_even_without_payload_bytes() {
    let mut client = client(b"{\"ev\":\"ok\",\"req\":1}\n{\"ev\":\"ok\",\"req\":2}\n");
    client
        .part_writer(1, "application/octet-stream", 0)
        .unwrap()
        .finish()
        .unwrap();
    client.pin(1, true).unwrap();
}

#[test]
fn an_unacknowledged_or_overlong_upload_prevents_later_requests() {
    for excess in [false, true] {
        let mut client = client(b"{\"ev\":\"ok\",\"req\":1}\n{\"ev\":\"ok\",\"req\":2}\n");
        {
            let mut writer = client
                .part_writer(1, "application/octet-stream", 4)
                .unwrap();
            writer.write_all(b"part").unwrap();
            if excess {
                assert_eq!(
                    writer.write(b"extra").unwrap_err().kind(),
                    io::ErrorKind::WriteZero
                );
            }
            // Finishing the byte count alone does not consume the part reply.
        }
        assert!(matches!(client.pin(1, true), Err(Error::Closed)));
    }
}

#[test]
fn a_matching_upload_refusal_preserves_the_connection() {
    let mut client = client(b"{\"ev\":\"error\",\"req\":1,\"code\":\"budget\",\"message\":\"unused\"}\n{\"ev\":\"ok\",\"req\":2}\n");
    let mut writer = client
        .part_writer(1, "application/octet-stream", 4)
        .unwrap();
    writer.write_all(b"part").unwrap();
    assert!(matches!(writer.finish(), Err(Error::Remote { code }) if code == "budget"));
    client.pin(1, true).unwrap();
}

#[test]
fn an_oversized_or_unterminated_frame_invalidates_the_connection() {
    for response in [
        vec![b' '; MAX_FRAME + 1],
        b"{\"ev\":\"ok\",\"req\":1}".to_vec(),
    ] {
        let mut client = client(&response);
        assert!(matches!(client.pin(1, true), Err(Error::InvalidFrame)));
        assert!(matches!(client.pin(1, true), Err(Error::Closed)));
    }
}

#[test]
fn a_mismatched_request_or_response_invalidates_the_connection() {
    for response in [
        b"{\"ev\":\"ok\",\"req\":2}\n".as_slice(),
        b"{\"ev\":\"blob\",\"req\":1,\"mime\":\"text/plain\",\"bytes\":8}\npayload!\n",
    ] {
        let mut client = client(response);
        assert!(matches!(
            client.pin(1, true),
            Err(Error::UnexpectedResponse)
        ));
        assert!(matches!(client.pin(1, true), Err(Error::Closed)));
    }
}

#[test]
fn a_matching_remote_error_does_not_desynchronize_a_later_request() {
    let mut client = client(b"{\"ev\":\"error\",\"req\":1,\"code\":\"future-code\",\"message\":\"unused\"}\n{\"ev\":\"ok\",\"req\":2}\n");
    assert!(matches!(client.pin(1, true), Err(Error::Remote { code }) if code == "future-code"));
    client.pin(2, true).unwrap();
}

#[test]
fn incomplete_incoming_and_outgoing_payloads_prevent_reuse() {
    let mut incoming =
        client(b"{\"ev\":\"blob\",\"req\":1,\"mime\":\"text/plain\",\"bytes\":8}\nshort");
    assert!(matches!(
        incoming.fetch_to(1, None, false, &mut Vec::new()),
        Err(Error::TruncatedPayload)
    ));
    assert!(matches!(incoming.pin(1, true), Err(Error::Closed)));

    let mut outgoing = client(b"{\"ev\":\"begin\",\"req\":1,\"draft\":1}\n");
    let draft = outgoing.begin(None).unwrap();
    assert!(matches!(
        outgoing.part_from(draft, "text/plain", 8, &mut b"short".as_slice()),
        Err(Error::TruncatedPayload)
    ));
    assert!(matches!(outgoing.commit(draft), Err(Error::Closed)));
}

#[test]
fn a_large_query_is_rejected_before_any_request_bytes_are_written() {
    let mut client = client(b"{\"ev\":\"ok\",\"req\":1}\n");
    assert!(matches!(
        client.list(ListQuery {
            query: Some(&"x".repeat(MAX_FRAME)),
            ..ListQuery::default()
        }),
        Err(Error::InvalidFrame)
    ));
    client.pin(1, true).unwrap();
}
