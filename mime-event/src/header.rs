use crate::debug::{dbg_single, ParamDbg};
use display_bytes::display_bytes_string;
use std::collections::HashMap;
use std::fmt;

/// Header contains information about an email header event
#[derive(PartialEq, Eq, Clone)]
pub enum Header<'a> {
    /// A header containing unstructured information as key, value
    Unstructured(&'a [u8], &'a [u8]),
    /// A Content-Type header
    ContentType {
        /// The MIME type
        mime_type: &'a [u8],
        /// Additional parameters to the MIME type
        parameters: HashMap<&'a [u8], Vec<u8>>,
    },
    /// Email From header
    From(&'a [u8]),
    /// Email To header
    To(&'a [u8]),
    /// An unparsed Email Date
    Date(&'a [u8]),
    /// Presentation information about a MIME part
    ContentDisposition {
        /// The type of disposition e.g "attachment"
        disposition_type: &'a [u8],
        /// Additional parameters to the disposition type e.g "filename"
        parameters: HashMap<&'a [u8], Vec<u8>>,
    },
    /// Description of a MIME part
    ContentDescription(&'a [u8]),
    /// Subject header
    Subject(&'a [u8]),
    /// The SMTP sender header
    Sender(&'a [u8]),
    /// Reply-To header
    ReplyTo(&'a [u8]),
    /// The Message-ID of the email message
    MessageId(&'a [u8]),
    /// End of the header
    End,
}

impl fmt::Debug for Header<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Header::Unstructured(key, value) => {
                let mut d = f.debug_struct("Unstructured");
                d.field("key", &display_bytes_string(key));
                d.field("value", &display_bytes_string(value));
                d.finish()
            }
            Header::From(from) => dbg_single(f, "from", from),
            Header::To(to) => dbg_single(f, "To", to),
            Header::Subject(subject) => dbg_single(f, "Subject", subject),
            Header::Sender(sender) => dbg_single(f, "Sender", sender),
            Header::ReplyTo(reply_to) => dbg_single(f, "ReplyTo", reply_to),
            Header::MessageId(message_id) => dbg_single(f, "MessageId", message_id),
            Header::Date(date) => dbg_single(f, "Date", date),
            Header::ContentDescription(desc) => dbg_single(f, "ContentDescription", desc),
            Header::ContentDisposition {
                disposition_type,
                parameters,
            } => {
                let mut d = f.debug_struct("ContentDisposition");
                d.field("disposition_type", &display_bytes_string(disposition_type));
                d.field("parameters", &ParamDbg(parameters));
                d.finish()
            }
            Header::ContentType {
                mime_type,
                parameters,
            } => {
                let mut d = f.debug_struct("ContentType");
                d.field("mime_type", &display_bytes_string(mime_type));
                d.field("parameters", &ParamDbg(parameters));
                d.finish()
            }
            Header::End => write!(f, "End"),
        }
    }
}
