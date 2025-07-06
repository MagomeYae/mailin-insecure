use mime_event::{Event, EventParser, Handler, Header, Multipart};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::io;

#[test]
fn multipart_alternative() {
    let msg = include_bytes!("multipart_alternative.msg");
    let handler = TestHandler::new(multipart_alternative_events());
    let handler = parse_message(&msg[..], handler).unwrap();
    handler.final_check()
}

#[test]
fn multipart_mixed() {
    let msg = include_bytes!("multipart_mixed.msg");
    let handler = TestHandler::new(multipart_mixed_events());
    let handler = parse_message(&msg[..], handler).unwrap();
    handler.final_check()
}

#[test]
fn swaks() {
    let msg = include_bytes!("swaks.msg");
    let handler = TestHandler::new(swaks_events());
    let handler = parse_message(&msg[..], handler).unwrap();
    handler.final_check()
}

struct TestHandler<'a> {
    current: usize,
    expected_events: Vec<Event<'a>>,
}

impl<'a> TestHandler<'a> {
    fn new(expected_events: Vec<Event<'a>>) -> Self {
        Self {
            current: 0,
            expected_events,
        }
    }

    fn final_check(&self) {
        assert_eq!(self.current, self.expected_events.len());
    }
}

impl<'a> Handler for TestHandler<'a> {
    fn event<'b>(&mut self, ev: Event) {
        if let Some(expected) = self.expected_events.get(self.current) {
            assert_eq!(*expected, ev);
        }
        self.current += 1;
    }
}

fn parse_message<'a>(message: &[u8], handler: TestHandler<'a>) -> io::Result<TestHandler<'a>> {
    let mut parser = EventParser::new(handler);
    for line in message.split(|ch| *ch == b'\n') {
        let mut buf = line.to_vec();
        buf.extend_from_slice(b"\r\n");
        parser
            .data(&buf)
            .map_err(|e| io::Error::other(format!("{e:?}")))?
    }
    Ok(parser.end())
}

fn multipart_alternative_events() -> Vec<Event<'static>> {
    vec![
        Event::Start,
        unstructured_header("X-sender", "<sender@sendersdomain.com>"),
        unstructured_header("X-receiver", "<somerecipient@recipientdomain.com>"),
        from(r#""Senders Name" <sender@sendersdomain.com>"#),
        to(r#""Recipient Name" <somerecipient@recipientdomain.com>"#),
        message_id("<5bec11c119194c14999e592feb46e3cf@sendersdomain.com>"),
        date("Sat, 24 Sep 2005 15:06:49 -0400"),
        subject("Sample Multi-Part"),
        unstructured_header("MIME-Version", "1.0"),
        content_type(
            "multipart/alternative",
            "boundary",
            "----=_NextPart_DC7E1BB5_1105_4DB3_BAE3_2A6208EB099D",
        ),
        Event::MultipartStart(Multipart::Alternative),
        Event::PartStart { offset: 507 },
        content_type("text/plain", "charset", "iso-8859-1"),
        unstructured_header("Content-Transfer-Encoding", "quoted-printable"),
        Event::BodyStart { offset: 600 },
        body("Sample Text Content\r\n"),
        Event::PartEnd { offset: 621 },
        Event::PartStart { offset: 676 },
        content_type("text/html", "charset", "iso-8859-1"),
        unstructured_header("Content-Transfer-Encoding", "quoted-printable"),
        Event::BodyStart { offset: 768 },
        body("<html>\r\n"),
        body("<head>\r\n"),
        body("</head>\r\n"),
        body("<body>\r\n"),
        body("  <div style=\"FONT-SIZE: 10pt; FONT-FAMILY: Arial\">Sample HTML =\r\n"),
        body("  Content</div>\r\n"),
        body("</body>\r\n"),
        body("</html>\r\n"),
        body("\r\n"),
        Event::PartEnd { offset: 904 },
        Event::MultipartEnd,
        Event::End,
    ]
}

fn multipart_mixed_events() -> Vec<Event<'static>> {
    vec![
        Event::Start,
        from("Moderator-Address"),
        to("Recipient-List"),
        date("Mon, 22 Mar 1994 13:34:51 +0000"),
        subject("Internet Digest, volume 42"),
        unstructured_header("MIME-Version", "1.0"),
        content_type("multipart/mixed", "boundary", "---- main boundary ----"),
        Event::MultipartStart(Multipart::Mixed),
        Event::PartStart { offset: 239 },
        Event::BodyStart { offset: 241 },
        body("...Introductory text or table of contents...\r\n"),
        body("\r\n"),
        Event::PartEnd { offset: 289 },
        Event::PartStart { offset: 316 },
        content_type("multipart/digest", "boundary", "---- next message ----"),
        Event::MultipartStart(Multipart::Digest),
        Event::PartStart { offset: 414 },
        Event::BodyStart { offset: 416 },
        body("From: someone-else\r\n"),
        body("Date: Fri, 26 Mar 1993 11:13:32 +0200\r\n"),
        body("Subject: my opinion\r\n"),
        body("\r\n"),
        body("...body goes here ...\r\n"),
        body("\r\n"),
        Event::PartEnd { offset: 523 },
        Event::PartStart { offset: 549 },
        Event::BodyStart { offset: 551 },
        body("From: someone-else-again\r\n"),
        body("Date: Fri, 26 Mar 1993 10:07:13 -0500\r\n"),
        body("Subject: my different opinion\r\n"),
        body("\r\n"),
        body("... another body goes here ...\r\n"),
        body("\r\n"),
        Event::PartEnd { offset: 683 },
        Event::MultipartEnd,
        Event::End,
    ]
}

fn swaks_events() -> Vec<Event<'static>> {
    vec![
        Event::Start,
        date("Fri, 04 Oct 2019 17:38:32 +0200"),
        to("saul@localhost"),
        from("saul@fish.localdomain"),
        subject("test Fri, 04 Oct 2019 17:38:32 +0200"),
        message_id("<20191004173832.005460@fish.localdomain>"),
        unstructured_header("X-Mailer", "swaks v20181104.0 jetmore.org/john/code/swaks/"),
        Event::BodyStart { offset: 249 },
        body("This is a test mailing\r\n"),
        body("\r\n"),
        body("\r\n"),
        body("\r\n"),
        Event::End,
    ]
}

//--- Helper functions to create events ---

fn unstructured_header<'a>(key: &'a str, value: &'a str) -> Event<'a> {
    Event::Header(Header::Unstructured(key.as_bytes(), value.as_bytes()))
}

fn header(header: Header) -> Event {
    Event::Header(header)
}

fn from(from: &str) -> Event {
    header(Header::From(from.as_bytes()))
}

fn to(to: &str) -> Event {
    header(Header::To(to.as_bytes()))
}

fn message_id(message_id: &str) -> Event {
    header(Header::MessageId(message_id.as_bytes()))
}

fn subject(subject: &str) -> Event {
    header(Header::Subject(subject.as_bytes()))
}

fn date(date: &str) -> Event {
    header(Header::Date(date.as_bytes()))
}

fn content_type<'a>(mime: &'a str, param_key: &'a str, param_value: &str) -> Event<'a> {
    Event::Header(Header::ContentType {
        mime_type: mime.as_bytes(),
        parameters: parameter_map(param_key, param_value),
    })
}

fn body(block: &str) -> Event {
    Event::Body(block.as_bytes())
}

fn parameter_map<'a>(key: &'a str, value: &str) -> HashMap<&'a [u8], Vec<u8>> {
    let mut ret = HashMap::new();
    ret.insert(key.as_bytes(), value.as_bytes().to_vec());
    ret
}
