//! The MIME event crate provides an event driven mime parser
//! and a higher level email message parser.
//!
//! This crate is intended to be used in mail servers. The parsers
//! parse one line at a time while writing to an object implementing
//! `std::io::Write`.
//!
//! # Examples
//! ## High level Message parser
//!```
//! use mime_event::{HeaderFields, Part, MessageParser};
//! # use std::io;
//! # use std::io::Write;
//! # use mailin::Data;
//!
//! // Create a message parser that writes to io::sink()
//! let mut parser = MessageParser.data_start("","",false,&[]).unwrap();
//!
//! // Write a message, one line at a time.
//! MessageParser.data(&mut parser, b"Subject: Example\r\n");
//! MessageParser.data(&mut parser, b"\r\n");
//!
//! // When there is no more input, call .end()
//! let message = MessageParser.data_end(parser).unwrap();
//!
//! // The returned Message object contains the parsed contents of the message
//! match message.top() {
//!     Some(
//!         Part{
//!             header: HeaderFields{
//!                 subject: Some(s), ..
//!             },
//!             ..
//!         }) => assert_eq!(s, b"Example"),
//!     _ => unreachable!(),
//! }
//! # Ok::<(), ()>(())
//! ```
//!## Low level Event parser
//! ```
//! use mime_event::{EventParser, Handler, Event, Header};
//! # use std::io;
//! # use std::io::Write;
//!
//! // Create a struct that will capture parsing events
//! #[derive(Default)]
//! struct MyHandler{
//!   subject: Vec<u8>,
//! }
//!
//! // Handle events as they arrive
//! impl Handler for MyHandler {
//!     fn event<'a>(&mut self, ev: Event<'a>) {
//!         match ev {
//!             Event::Header(Header::Subject(s)) => self.subject = s.to_vec(),
//!             _ => (),
//!         }
//!     }
//! }
//!
//! // Create an event driven parser
//! let mut parser = EventParser::new(MyHandler::default());
//!
//! // Write a message, one line at a time.
//! parser.data(b"Subject: Example\r\n");
//! parser.data(b"\r\n");
//!
//! // When there is no more input, call .end()
//! let handler = parser.end();
//! assert_eq!(&handler.subject, b"Example")
//! ```

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

mod debug;
mod event;
mod header;
mod header_buffer;
mod line_parser;
mod message;
mod message_handler;
mod message_parser;
mod parser;

pub use event::{Event, Mime, Multipart};
pub use header::Header;
pub use message::{HeaderFields, Message, Part};
pub use message_handler::MessageHandler;
pub use message_parser::{MessageParser, MessageParserData};
pub use parser::{EventParser, Handler};
