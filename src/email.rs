use lettre::{self, EmailAddress};
use mail_parser::Message;

use crate::Error;

#[derive(Debug, PartialEq)]
pub(crate) struct Email<'a> {
    pub(crate) message_id: String,
    pub(crate) raw: &'a [u8],
    parsed_message: Message<'a>,
}

impl<'a> Email<'a> {
    fn parse(raw: &'a [u8]) -> Result<Email<'a>, Error> {
        if let Some(parsed_message) = Message::parse(raw) {
            if let Some(id) = parsed_message.get_message_id() {
                Ok(Email {
                    message_id: id.to_string(),
                    raw,
                    parsed_message,
                })
            } else {
                Err(Error::MailParsing("Missing message-id header."))
            }
        } else {
            Err(Error::MailParsing(
                "Could not parse RFC5322/RFC822 message.",
            ))
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct SmtpEmail<'b> {
    pub(crate) from: Option<EmailAddress>,
    pub(crate) to: Vec<EmailAddress>,
    pub(crate) content: Email<'b>,
}

impl<'b> SmtpEmail<'b> {
    pub(crate) fn new(
        from: Option<EmailAddress>,
        to: Vec<EmailAddress>,
        data: &'b [u8],
    ) -> Result<SmtpEmail<'b>, Error> {
        Ok(SmtpEmail {
            from,
            to,
            content: Email::parse(data)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lettre::{self, SendableEmail};

    impl<'a> SmtpEmail<'a> {
        /// Converts a `lettre::SendableEmail` to a `SmtpEmail`.
        /// This may panic, if the `message` of `m` is a `Reader`, that returns an `io::Error`.
        pub fn from_tokio_mail(m: SendableEmail, buf: &'a mut Vec<u8>) -> Self {
            let from = m.envelope().from().cloned();
            let to = m.envelope().to().to_vec();
            let message_id = format!("{}.lettre@localhost", m.message_id());
            match m.message() {
                lettre::Message::Bytes(curs) => {
                    buf.extend_from_slice(curs.into_inner().as_slice());
                }
                lettre::Message::Reader(mut r) => {
                    r.read_to_end(buf)
                        .expect("Called SmtpEmail::from() with a Reader, that returned an Error.");
                }
            };
            // We add another CRLF at the end, to allow for a comparison with received mails:
            buf.push(0x0d);
            buf.push(0x0a);

            Self {
                from,
                to,
                content: Email {
                    message_id,
                    raw: buf.as_slice(),
                    parsed_message: Message::parse(buf.as_slice())
                        .expect("Could not parse message."),
                },
            }
        }
    }
}
