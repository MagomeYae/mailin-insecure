pub use crate::header::Header;
use display_bytes::display_bytes;
use std::fmt;
use std::str;

/// Events sent to a Handler
#[derive(PartialEq, Eq)]
pub enum Event<'a> {
    /// Parsing has Started
    Start,
    /// Header line
    Header(Header<'a>),
    /// Start of a MIME multipart entity
    MultipartStart(Multipart),
    /// Start of a MIME mulitpart part
    PartStart {
        /// Byte offset of the Part in the mail message
        offset: usize,
    },
    /// Start of a Body
    BodyStart {
        /// Byte offset of the Body in the mail message
        offset: usize,
    },
    /// A line of an email body
    Body(&'a [u8]),
    /// End of a MIME mulitpart part
    PartEnd {
        /// Byte offset of the end of the Part in the mail message
        offset: usize,
    },
    /// End of a MIME multipart entity
    MultipartEnd,
    /// Parsing has finished
    End,
}

/// A MIME type
#[derive(Debug, Clone)]
pub enum Mime {
    /// A multipart MIME type
    Multipart(Multipart),
    /// Not a multipart MIME type
    Type(Vec<u8>),
}

/// Multipart MIME types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Multipart {
    /// MIME multipart alternative e.g text vs html
    Alternative,
    /// MIME multipart mixed content types. Usually inline.
    Mixed,
    /// MIME multipart message digest
    Digest,
}

pub(crate) fn mime_type(v: &[u8]) -> Mime {
    let lower = str::from_utf8(v).map(|s| s.to_lowercase());
    if let Ok(s) = lower {
        match s.as_str() {
            "multipart/alternative" => Mime::Multipart(Multipart::Alternative),
            "multipart/mixed" => Mime::Multipart(Multipart::Mixed),
            "multipart/digest" => Mime::Multipart(Multipart::Digest),
            _ => Mime::Type(v.to_vec()),
        }
    } else {
        Mime::Type(v.to_vec())
    }
}

impl fmt::Debug for Event<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::Start => write!(f, "Start"),
            Event::Header(header) => write!(f, "Header({header:?})"),
            Event::MultipartStart(multipart) => write!(f, "MultipartStart({multipart:?})"),
            Event::PartStart { offset } => write!(f, "PartStart({offset:?})"),
            Event::BodyStart { offset } => write!(f, "BodyStart({offset:?})"),
            Event::Body(block) => write!(f, "Body({})", display_bytes(block)),
            Event::PartEnd { offset } => write!(f, "PartEnd({offset:?})"),
            Event::MultipartEnd => write!(f, "MultipartEnd"),
            Event::End => write!(f, "End"),
        }
    }
}
