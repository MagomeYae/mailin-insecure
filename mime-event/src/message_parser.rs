use crate::message::Message;
use crate::message_handler::MessageHandler;
use crate::parser::EventParser;
use mailin::{Data, Response};

/// Wraps an event parser to parse messages
/// # Example
/// ```
/// use mime_event::MessageParser;
/// # use std::io;
/// # use std::io::Write;
/// use mailin::Data;
///
/// // Create a message parser that writes to io::sink()
/// let mut parser = MessageParser.data_start("","",false,&[]).unwrap();
///
/// // Write a message, one line at a time.
/// MessageParser.data(&mut parser, b"Subject: Example\r\n");
/// MessageParser.data(&mut parser,b"\r\n");
///
/// // When there is no more input, call .end()
/// let message = MessageParser.data_end(parser).unwrap();
///
/// // The returned Message object contains the parsed contents of the message
/// let header = &message.top().unwrap().header;
/// assert_eq!(header.subject.as_ref().unwrap(), b"Example");
/// # Ok::<(), ()>(())
/// ```
#[derive(Clone)]
pub struct MessageParser;

/// TODO
pub struct MessageParserData {
    parser: EventParser<MessageHandler>,
}

/// Write data to the MessageParser to parse a Message
impl Data for MessageParser {
    type State = MessageParserData;
    type Output = Message;

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Result<Self::State, Response> {
        Ok(MessageParserData {
            parser: EventParser::new(MessageHandler::default()),
        })
    }

    fn data(&mut self, state: &mut Self::State, buf: &[u8]) -> Result<(), Response> {
        state.parser.data(buf)
    }

    fn data_end(&mut self, state: Self::State) -> Result<Self::Output, Response> {
        Ok(state.parser.end().get_message())
    }
}
