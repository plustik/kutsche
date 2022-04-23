use lettre::EmailAddress;
use mailin::{response, Handler, Response, SessionBuilder};

use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::net::{IpAddr, TcpListener, ToSocketAddrs};
use std::sync::mpsc::{channel, Sender};

use crate::{email::SmtpEmail, Error};

#[cfg(test)]
mod tests;

pub(crate) struct SmtpServer {
    tcp_listener: TcpListener,
    session_builder: SessionBuilder,
}

impl SmtpServer {
    pub(crate) fn new<A: ToSocketAddrs>(local_addr: A) -> Result<Self, Error> {
        Ok(SmtpServer {
            tcp_listener: TcpListener::bind(local_addr)?,
            session_builder: SessionBuilder::new("TCP mail saver"),
        })
    }

    pub(crate) fn recv_mail(&self) -> Result<SmtpEmail, Error> {
        let (stream, peer_addr) = self.tcp_listener.accept()?;
        let mut conn_buf_read = BufReader::new(&stream);
        let mut conn_buf_write = BufWriter::new(&stream);

        let (sender, receiver) = channel();
        let mut session = self
            .session_builder
            .build(peer_addr.ip(), MailHandler::new(sender));

        session.greeting().write_to(&mut conn_buf_write)?;
        conn_buf_write.flush()?;
        let mut buf = String::new();
        let mut ongoing_communication = true;
        while ongoing_communication {
            buf.clear();
            conn_buf_read.read_line(&mut buf)?;
            let resp = session.process(buf.as_bytes());
            ongoing_communication = resp.action != response::Action::Close;
            resp.write_to(&mut conn_buf_write)?;
            conn_buf_write.flush()?;
        }

        Ok(receiver.recv().expect("Receive email channel hung up."))
    }
}

struct MailHandler {
    from: Option<EmailAddress>,
    to: Vec<EmailAddress>,
    msg_buf: Option<Vec<u8>>,
    result_sender: Sender<SmtpEmail>,
}

impl MailHandler {
    fn new(sender: Sender<SmtpEmail>) -> Self {
        MailHandler {
            from: None,
            to: vec![],
            msg_buf: None,
            result_sender: sender,
        }
    }
}

impl Handler for MailHandler {
    fn helo(&mut self, _ip: IpAddr, _domain: &str) -> Response {
        response::OK
    }

    fn mail(&mut self, _ip: IpAddr, _domain: &str, from: &str) -> Response {
        self.from =
            Some(EmailAddress::new(String::from(from)).expect("Invalid FROM email address."));
        response::OK
    }

    fn rcpt(&mut self, to: &str) -> Response {
        self.to
            .push(EmailAddress::new(String::from(to)).expect("Invalid TO email address."));
        response::OK
    }

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Response {
        self.msg_buf = Some(vec![]);
        response::OK
    }

    fn data(&mut self, buf: &[u8]) -> io::Result<()> {
        self.msg_buf
            .as_mut()
            .expect("Received data but no data command.")
            .extend_from_slice(buf);
        Ok(())
    }

    fn data_end(&mut self) -> Response {
        let complete_mail = SmtpEmail::new(
            self.from.take(),
            self.to.drain(0..).collect(),
            self.msg_buf
                .take()
                .expect("Received DATA_END before DATA_START."),
        )
        .expect("Could not parse received message.");
        self.result_sender
            .send(complete_mail)
            .expect("Could not send received mail through channel.");

        response::OK
    }

    fn auth_plain(
        &mut self,
        _authorization_id: &str,
        _authentication_id: &str,
        _password: &str,
    ) -> Response {
        response::INVALID_CREDENTIALS
    }
}
