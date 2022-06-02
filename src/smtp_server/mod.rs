use lettre::EmailAddress;
use log::info;
use mailin::{response, Handler, Response, SessionBuilder};
use rustls::{ServerConfig, ServerConnection};

use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{IpAddr, TcpListener};
use std::sync::{
    mpsc::{channel, Sender},
    Arc,
};

use crate::{config::Config, email::SmtpEmail, Error};

#[cfg(test)]
mod tests;

pub(crate) struct SmtpServer {
    tcp_listener: TcpListener,
    session_builder: SessionBuilder,
    tls_config: Option<Arc<ServerConfig>>,
}

impl SmtpServer {
    pub(crate) fn new(conf: &Config) -> Result<Self, Error> {
        Ok(SmtpServer {
            tcp_listener: TcpListener::bind(conf.local_addr)?,
            session_builder: SessionBuilder::new("TCP mail saver"),
            tls_config: conf.tls_config.clone(),
        })
    }

    pub(crate) fn recv_mail(&self) -> Result<SmtpEmail, Error> {
        if self.tls_config.is_some() {
            self.recv_mail_tls()
        } else {
            self.recv_mail_plain()
        }
    }

    fn recv_mail_plain(&self) -> Result<SmtpEmail, Error> {
        let (stream, peer_addr) = self.tcp_listener.accept()?;
        info!("Accepted incoming TCP connection.");

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

    fn recv_mail_tls(&self) -> Result<SmtpEmail, Error> {
        let (mut tcp_stream, peer_addr) = self.tcp_listener.accept()?;
        info!("Accepted incoming TCP connection.");
        let mut tls_conn = ServerConnection::new(
            self.tls_config
                .as_ref()
                .expect("recv_mail_tls() was called, but there is no tls_config.")
                .clone(),
        )?;

        let (sender, receiver) = channel();
        let mut session = self
            .session_builder
            .build(peer_addr.ip(), MailHandler::new(sender));

        let mut last_resp = session.greeting();
        last_resp.write_to(&mut tls_conn.writer())?;

        let mut in_buf = Vec::new();
        while last_resp.action != response::Action::Close {
            tls_conn.complete_io(&mut tcp_stream)?;
            match tls_conn.process_new_packets() {
                Ok(state) => {
                    // Process newly received data:
                    if state.plaintext_bytes_to_read() > 0 {
                        tls_conn.reader().read_to_end(&mut in_buf)?;
                        // Process each line and save last unfinished line:
                        let mut offset = 0;
                        while offset < in_buf.len() {
                            let mut end = offset;
                            // Find next \r\n (0xa, 0xd)
                            while end < in_buf.len()
                                && (in_buf[end - 1] != 0xd || in_buf[end] != 0xa)
                            {
                                end += 1;
                            }
                            if in_buf[end - 1] == 0xd && in_buf[end] == 0xa {
                                end += 1;
                                // Process line:
                                last_resp = session.process(&in_buf[offset..end]);
                                let mut out_buf = Vec::new();
                                last_resp.write_to(&mut out_buf)?;
                                tls_conn.writer().write_all(out_buf.as_slice())?;
                            }
                            offset = end;
                        }
                        // Remove the processed lines from the buffer:
                        in_buf.drain(0..offset);
                    }
                    // Check, if the peer closed the connection:
                    if state.peer_has_closed() {
                        break;
                    }
                }

                Err(e) => {
                    if tls_conn.wants_write() {
                        tls_conn.write_tls(&mut tcp_stream)?;
                    }
                    return Err(e.into());
                }
            }
        }
        // Close connection and send remaining buffer:
        tls_conn.send_close_notify();
        while tls_conn.wants_write() {
            tls_conn.write_tls(&mut tcp_stream)?;
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
        info!("Received an email over SMTP.");
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
