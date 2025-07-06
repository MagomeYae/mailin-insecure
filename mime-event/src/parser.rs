use crate::event::*;
use crate::header::Header;
use crate::header_buffer::HeaderBuffer;
use crate::line_parser;
use mailin::Response;
use std::collections::HashMap;

/// A Handler receives parser events
pub trait Handler {
    /// Method that receives parser events
    fn event(&mut self, ev: Event);
}

#[derive(Clone, Copy, Debug)]
enum State {
    Start,
    Header,
    MultipartHeader,
    MultipartPreamble,
    PartStart,
    Body,
}

struct MultipartState {
    content_type: Multipart,
    boundary: Vec<u8>,
}

/// EventParser is an event driven email parser.
/// # Example
/// ```
/// use mime_event::{EventParser, Handler, Event, Header};
///
/// // Create a struct that will capture parsing events
/// #[derive(Default)]
/// struct MyHandler{
///   subject: Vec<u8>,
/// }
///
/// // Handle events as they arrive
/// impl Handler for MyHandler {
///     fn event<'a>(&mut self, ev: Event<'a>) {
///         match ev {
///             Event::Header(Header::Subject(s)) => self.subject = s.to_vec(),
///             _ => (),
///         }
///     }
/// }
///
/// // Create an event driven parser
/// let mut parser = EventParser::new(MyHandler::default());
///
/// // Write a message, one line at a time.
/// parser.data(b"Subject: Example\r\n");
/// parser.data(b"\r\n");
///
/// // When there is no more input, call .end()
/// let handler = parser.end();
/// assert_eq!(&handler.subject, b"Example");
/// ```
pub struct EventParser<H: Handler> {
    state: State,
    offset: usize,
    handler: H,
    content_type: Mime,
    boundary: Option<Vec<u8>>,
    multipart_stack: Vec<MultipartState>,
    header_buffer: HeaderBuffer,
}

impl<H: Handler> EventParser<H> {
    /// Create a new EventParser.
    /// Writing to the EventParser will write to the writer.
    /// Writing to the EventParser will produce events that are sent to the handler.
    pub fn new(handler: H) -> Self {
        Self {
            state: State::Start,
            offset: 0,
            handler,
            content_type: Mime::Type(b"text/plain".to_vec()),
            boundary: None,
            multipart_stack: Vec::default(),
            header_buffer: HeaderBuffer::default(),
        }
    }

    /// Call when message has finished and there is no more input.
    /// Returns the handler.
    pub fn end(mut self) -> H {
        self.handler.event(Event::End);
        self.handler
    }

    fn is_open_boundary(&self, buf: &[u8]) -> bool {
        self.boundary
            .as_ref()
            .filter(|b| buf.starts_with(b))
            .is_some()
    }

    fn is_close_boundary(&self, buf: &[u8]) -> bool {
        self.boundary
            .as_ref()
            .filter(|b| {
                let end = b.len();
                buf.starts_with(b) && buf.len() > end + 2 && buf.ends_with(b"--\r\n")
            })
            .is_some()
    }

    fn header_field(&mut self, buf: &[u8], state: State) -> Result<State, Response> {
        if buf.starts_with(b"\r\n") {
            self.state = match state {
                State::MultipartHeader => State::MultipartPreamble,
                _ => {
                    self.handler.event(Event::BodyStart {
                        offset: self.offset + 2,
                    });
                    State::Body
                }
            };
            Ok(self.state)
        } else {
            let token = line_parser::header(buf)?;
            if let Header::ContentType {
                mime_type: mtype,
                parameters: params,
            } = token.clone()
            {
                self.content_type(mtype, params);
            }
            self.handler.event(Event::Header(token));
            if let Mime::Multipart(_) = self.content_type {
                Ok(State::MultipartHeader)
            } else {
                Ok(state)
            }
        }
    }

    // Handle Content-Type headers
    fn content_type(&mut self, mtype: &[u8], params: HashMap<&[u8], Vec<u8>>) {
        if let (Mime::Multipart(m), Some(b)) = (&self.content_type, &self.boundary) {
            self.multipart_stack.push(MultipartState {
                content_type: *m,
                boundary: b.clone(),
            })
        }
        self.content_type = mime_type(mtype);
        if let Mime::Multipart(_) = &self.content_type {
            self.boundary = params.get(&(b"boundary")[..]).map(|boundary| {
                let mut full = b"--".to_vec();
                full.extend_from_slice(boundary);
                full
            });
        }
    }

    // Called when data is written to the writer
    fn handle_write(&mut self, buf: &[u8]) -> Result<(), Response> {
        match self.state {
            State::Start => {
                self.handler.event(Event::Start);
                self.state = State::Header;
                self.handle_header(buf)
            }
            State::Header | State::MultipartHeader | State::PartStart => self.handle_header(buf),
            _ => self.handle_line(buf, buf.len()),
        }
    }

    fn handle_header(&mut self, buf: &[u8]) -> Result<(), Response> {
        if buf.starts_with(b"\r\n") {
            if let Some((line, length)) = self.header_buffer.take() {
                self.handle_line(&line, length)?;
            }
            self.handle_line(buf, buf.len())
        } else if let Some((line, length)) = self.header_buffer.next_line(buf) {
            self.handle_line(&line, length)
        } else {
            Ok(())
        }
    }

    // Called when a complete line of data is available
    fn handle_line(&mut self, buf: &[u8], buf_len: usize) -> Result<(), Response> {
        let next_state = match self.state {
            State::Start => unreachable!(),
            State::MultipartHeader => self.header_field(buf, State::MultipartHeader)?,
            State::Header => self.header_field(buf, State::Header)?,
            State::PartStart => {
                self.handler.event(Event::PartStart {
                    offset: self.offset,
                });
                self.header_field(buf, State::Header)?
            }
            State::MultipartPreamble => {
                if self.is_open_boundary(buf) {
                    if let Mime::Multipart(m) = self.content_type {
                        self.handler.event(Event::MultipartStart(m));
                    }
                    State::PartStart
                } else {
                    State::MultipartPreamble
                }
            }
            State::Body => {
                if self.is_close_boundary(buf) {
                    self.handler.event(Event::PartEnd {
                        offset: self.offset,
                    });
                    self.handler.event(Event::MultipartEnd);
                    // Use last multipart if available
                    if let Some(last) = self.multipart_stack.pop() {
                        self.content_type = Mime::Multipart(last.content_type);
                        self.boundary = Some(last.boundary);
                    }
                    State::Header
                } else if self.is_open_boundary(buf) {
                    self.handler.event(Event::PartEnd {
                        offset: self.offset,
                    });
                    State::PartStart
                } else {
                    self.handler.event(Event::Body(buf));
                    State::Body
                }
            }
        };
        self.state = next_state;
        self.offset += buf_len;
        Ok(())
    }

    /// TODO
    pub fn data(&mut self, buf: &[u8]) -> Result<(), Response> {
        self.handle_write(buf)
    }
}
