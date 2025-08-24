use crate::header::Header;
use nom::branch::alt;
use nom::bytes::complete::{is_not, tag, tag_no_case, take_while1};
use nom::combinator::{map, recognize};
use nom::multi::fold_many0;
use nom::sequence::{pair, preceded, terminated};
use nom::IResult;
use std::collections::HashMap;
use std::io;

// Parse a header line.
// The result type must be io::Result to be compatible with io::Write()
pub(crate) fn header(line: &[u8]) -> io::Result<Header<'_>> {
    let res = alt((
        header_end,
        content,
        from,
        to,
        subject,
        sender,
        reply_to,
        message_id,
        date,
        content_disposition,
        content_description,
        unstructured,
    ))(line);
    match res {
        Ok((_, header)) => Ok(header),
        Err(err) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{err:?}"),
        )),
    }
}

fn content(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(header_with_params(b"Content-Type"), |v| {
        Header::ContentType {
            mime_type: v.0,
            parameters: v.1,
        }
    })(buf)
}

fn content_disposition(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(header_with_params(b"Content-Disposition"), |v| {
        Header::ContentDisposition {
            disposition_type: v.0,
            parameters: v.1,
        }
    })(buf)
}

type HeaderParams<'a> = IResult<&'a [u8], (&'a [u8], HashMap<&'a [u8], Vec<u8>>)>;

// Parse a header field followed by parameters
fn header_with_params(header: &[u8]) -> impl Fn(&[u8]) -> HeaderParams + '_ {
    move |buf: &[u8]| {
        let preamble = match_header_key(header);
        let (i, value) = preceded(preamble, header_value_with_parameters)(buf)?;
        let mut parameter_parser = terminated(parameters, tag(b"\r\n"));
        let (i, params) = parameter_parser(i)?;
        Ok((i, (value, params)))
    }
}

fn match_header_key(header: &[u8]) -> impl Fn(&[u8]) -> IResult<&[u8], &[u8]> + '_ {
    move |buf: &[u8]| terminated(tag_no_case(header), colon_space)(buf)
}

fn colon_space(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    recognize(pair(tag(b":"), space))(buf)
}

fn space(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    take_while1(|c| c == b' ')(buf)
}

fn header_value_with_parameters(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    is_not(";\r\n")(buf)
}

fn parameters(buf: &[u8]) -> IResult<&[u8], HashMap<&[u8], Vec<u8>>> {
    fold_many0(parameter, HashMap::new, |mut acc: HashMap<_, _>, item| {
        acc.insert(item.0, item.1);
        acc
    })(buf)
}

fn parameter(buf: &[u8]) -> IResult<&[u8], (&[u8], Vec<u8>)> {
    let preamble = pair(tag(b";"), space);
    let (i, attribute) = preceded(preamble, token)(buf)?;
    let (i, value) = preceded(tag(b"="), parameter_value)(i)?;
    Ok((i, (attribute, value)))
}

fn parameter_value(buf: &[u8]) -> IResult<&[u8], Vec<u8>> {
    let token_vec = map(token, |b: &[u8]| b.to_vec());
    alt((token_vec, quoted_string))(buf)
}

fn token(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    take_while1(|c| c != b' ' && !tspecial(c) && !ctl(c))(buf)
}

fn quoted_string(buf: &[u8]) -> IResult<&[u8], Vec<u8>> {
    let qs = preceded(tag(b"\""), in_quotes);
    terminated(qs, tag(b"\""))(buf)
}

fn in_quotes(buf: &[u8]) -> IResult<&[u8], Vec<u8>> {
    let mut ret = Vec::new();
    let mut i = 0;
    while i < buf.len() && buf[i] != b'"' {
        if buf[i] == b'\\' {
            i += 1;
        }
        ret.push(buf[i]);
        i += 1;
    }
    Ok((&buf[i..], ret))
}

fn ctl(c: u8) -> bool {
    c == b'\r' || c == b'\n'
}

fn tspecial(c: u8) -> bool {
    c == b'('
        || c == b')'
        || c == b'<'
        || c == b'>'
        || c == b'@'
        || c == b','
        || c == b';'
        || c == b':'
        || c == b'\\'
        || c == b'"'
        || c == b'/'
        || c == b'['
        || c == b']'
        || c == b'?'
        || c == b'='
}

fn match_unstructured(header: &[u8]) -> impl Fn(&[u8]) -> IResult<&[u8], &[u8]> + '_ {
    move |buf: &[u8]| {
        let value_parser = preceded(match_header_key(header), unstructured_value);
        terminated(value_parser, tag(b"\r\n"))(buf)
    }
}

fn from(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"From"), Header::From)(buf)
}

fn to(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"To"), Header::To)(buf)
}

fn subject(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"Subject"), Header::Subject)(buf)
}

fn sender(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"Sender"), Header::Sender)(buf)
}

fn reply_to(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"Reply-To"), Header::ReplyTo)(buf)
}

fn message_id(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"Message-ID"), Header::MessageId)(buf)
}

fn date(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"Date"), Header::Date)(buf)
}

fn content_description(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(match_unstructured(b"Content-Description"), |v| {
        Header::ContentDescription(v)
    })(buf)
}

fn unstructured(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    let (i, key) = terminated(header_key, colon_space)(buf)?;
    let (i, value) = terminated(unstructured_value, tag(b"\r\n"))(i)?;
    Ok((i, Header::Unstructured(key, value)))
}

fn header_key(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    take_while1(|c| c != b':' && c != b' ')(buf)
}

fn unstructured_value(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    is_not("\r\n")(buf)
}

fn header_end(buf: &[u8]) -> IResult<&[u8], Header<'_>> {
    map(tag(b"\r\n"), |_| Header::End)(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;
    use pretty_assertions::assert_eq;

    #[test]
    fn unstructured_header() {
        let tok = header(b"X-sender: <sender@sendersdomain.com>\r\n").unwrap();
        assert_eq!(
            tok,
            Header::Unstructured(b"X-sender", b"<sender@sendersdomain.com>")
        )
    }

    #[test]
    fn end_header() {
        let tok = header(b"\r\n").unwrap();
        assert_eq!(tok, Header::End)
    }

    #[test]
    fn case_insensitive_header() {
        let tok = header(b"Message-Id: <20191004173832.005460@fish.localdomain>\r\n").unwrap();
        assert_eq!(
            tok,
            Header::MessageId(b"<20191004173832.005460@fish.localdomain>")
        )
    }

    #[test]
    fn content_type() {
        let tok = header(b"Content-Type: multipart/mixed; boundary=--boundary--\r\n").unwrap();
        let expected_params = hashmap! {
            b"boundary".as_ref() => b"--boundary--".to_vec(),
        };
        assert_eq!(
            tok,
            Header::ContentType {
                mime_type: b"multipart/mixed",
                parameters: expected_params,
            }
        )
    }

    #[test]
    fn content_disposition() {
        let mut line = br#"Content-Disposition: attachment; filename=genome.jpeg; modification-date="Wed, 12 Feb 1997 16:29:51 -0500""#.to_vec();
        line.extend_from_slice(b"\r\n");
        let tok = header(&line).unwrap();
        let expected_params = hashmap! {
            b"filename".as_ref() => b"genome.jpeg".to_vec(),
            b"modification-date".as_ref() => b"Wed, 12 Feb 1997 16:29:51 -0500".to_vec(),
        };
        assert_eq!(
            tok,
            Header::ContentDisposition {
                disposition_type: b"attachment",
                parameters: expected_params,
            }
        )
    }

    #[test]
    fn quoted_boundary() {
        let tok =
            header(b"Content-Type: multipart/mixed; boundary=\"-- boundary --\"\r\n").unwrap();
        let expected_params = hashmap! {
            b"boundary".as_ref() => b"-- boundary --".to_vec(),
        };
        assert_eq!(
            tok,
            Header::ContentType {
                mime_type: b"multipart/mixed",
                parameters: expected_params,
            }
        )
    }
}
