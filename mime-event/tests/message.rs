use mailin::Data;
use mime_event::{HeaderFields, Message, MessageParser};
use pretty_assertions::assert_eq;
use std::io;

#[test]
fn multipart_alternative() {
    let msg = include_bytes!("multipart_alternative.msg");
    let message = parse_message(&msg[..]).unwrap();
    let expected_header = HeaderFields {
        message_id: field(b"<5bec11c119194c14999e592feb46e3cf@sendersdomain.com>"),
        from: field(br#""Senders Name" <sender@sendersdomain.com>"#),
        to: field(br#""Recipient Name" <somerecipient@recipientdomain.com>"#),
        date: field(b"Sat, 24 Sep 2005 15:06:49 -0400"),
        reply_to: None,
        sender: None,
        subject: field(b"Sample Multi-Part"),
    };
    let header = &message.text().unwrap().header;
    assert_eq!(header, &expected_header);
    let header = &message.top().unwrap().header;
    assert_eq!(header, &expected_header);
    assert_eq!(message.top().unwrap().body(), (600, 22));
    assert_eq!(message.html().unwrap().body(), (768, 137));
}

#[test]
fn multipart_mixed() {
    let msg = include_bytes!("multipart_mixed.msg");
    let message = parse_message(&msg[..]).unwrap();
    let expected_top_header = HeaderFields {
        message_id: None,
        from: field(b"Moderator-Address"),
        to: field(b"Recipient-List"),
        date: field(b"Mon, 22 Mar 1994 13:34:51 +0000"),
        reply_to: None,
        sender: None,
        subject: field(b"Internet Digest, volume 42"),
    };
    let top_header = &message.top().unwrap().header;
    assert_eq!(top_header, &expected_top_header);
    assert_eq!(message.top().unwrap().body(), (241, 49));
    for (i, attachment) in message.attachments().enumerate() {
        match i {
            0 => assert_eq!(attachment.position(), (414, 110)),
            1 => assert_eq!(attachment.position(), (549, 135)),
            _ => panic!("Unexpected attachment"),
        }
    }
}

#[test]
fn swaks() {
    let msg = include_bytes!("swaks.msg");
    let message = parse_message(&msg[..]).unwrap();
    let expected_header = HeaderFields {
        message_id: field(b"<20191004173832.005460@fish.localdomain>"),
        from: field(b"saul@fish.localdomain"),
        to: field(b"saul@localhost"),
        date: field(b"Fri, 04 Oct 2019 17:38:32 +0200"),
        reply_to: None,
        sender: None,
        subject: field(b"test Fri, 04 Oct 2019 17:38:32 +0200"),
    };
    let header = &message.top().unwrap().header;
    assert_eq!(header, &expected_header);
    let header = &message.text().unwrap().header;
    assert_eq!(header, &expected_header);
}

fn field(value: &[u8]) -> Option<Vec<u8>> {
    Some(value.to_vec())
}

fn parse_message(message: &[u8]) -> io::Result<Message> {
    let mut parser = MessageParser;
    let mut data = parser
        .data_start("", "", false, &[])
        .map_err(|e| io::Error::other(format!("{e:?}")))?;
    for line in message.split(|ch| *ch == b'\n') {
        let mut buf = line.to_vec();
        buf.extend_from_slice(b"\r\n");
        parser
            .data(&mut data, &buf)
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
    }
    parser
        .data_end(data)
        .map_err(|e| io::Error::other(format!("{e:?}")))
}
