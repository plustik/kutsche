use lettre::{self, EmailAddress};
use mail_parser::Message;

use crate::Error;

#[derive(Debug, PartialEq)]
pub(crate) struct Email {
    pub(crate) message_id: String,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SmtpEmail {
    pub(crate) from: Option<EmailAddress>,
    pub(crate) to: Vec<EmailAddress>,
    pub(crate) content: Email,
}

impl SmtpEmail {
    pub(crate) fn new(
        from: Option<EmailAddress>,
        to: Vec<EmailAddress>,
        data: Vec<u8>,
    ) -> Result<Self, Error> {
        let message_id = if let Some(p) = Message::parse(data.as_slice()) {
            if let Some(id) = p.get_message_id() {
                id.to_string()
            } else {
                return Err(Error::Parsing("Missing message-id header."));
            }
        } else {
            return Err(Error::Parsing("Could not parse RFC5322/RFC822 message."));
        };

        Ok(SmtpEmail {
            from,
            to,
            content: Email { message_id, data },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lettre::{self, SendableEmail};
    use lettre_email;

    impl From<SendableEmail> for SmtpEmail {
        /// Converts a `lettre::SendableEmail` to a `SmtpEmail`.
        /// This may panic, if the `message` of `m` is a `Reader`, that returns an `io::Error`.
        fn from(m: SendableEmail) -> Self {
            let from = m.envelope().from().cloned();
            let to = m.envelope().to().to_vec();
            let message_id = format!("{}.lettre@localhost", m.message_id());
            let mut data = match m.message() {
                lettre::Message::Bytes(curs) => curs.into_inner(),
                lettre::Message::Reader(mut r) => {
                    let mut buf = vec![];
                    r.read_to_end(&mut buf)
                        .expect("Called SmtpEmail::from() with a Reader, that returned an Error.");
                    buf
                }
            };
            // We add another CRLF at the end, to allow for a comparison with received mails:
            data.push(0x0d);
            data.push(0x0a);

            Self {
                from,
                to,
                content: Email { message_id, data },
            }
        }
    }

    impl From<lettre_email::Email> for SmtpEmail {
        fn from(m: lettre_email::Email) -> Self {
            Self::from(Into::<lettre::SendableEmail>::into(m))
        }
    }
}
