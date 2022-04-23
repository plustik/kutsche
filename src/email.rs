use lettre::{self, EmailAddress};
use mail_parser::Message;

use crate::Error;

#[derive(Debug, PartialEq)]
pub(crate) struct SmtpEmail {
    from: Option<EmailAddress>,
    to: Vec<EmailAddress>,
    message_id: String,
    data: Vec<u8>,
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
            message_id,
            data,
        })
    }
}
