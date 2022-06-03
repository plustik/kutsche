use lettre::EmailAddress;
use log::info;
use mailin::{response, Handler, Response, SessionBuilder};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufStream},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

use std::net::{IpAddr, SocketAddr};
use std::sync::mpsc::{channel, Sender};

use crate::{config::Config, email::SmtpEmail, Error};

#[cfg(test)]
mod tests;

pub(crate) struct SmtpServer {
    tcp_listener: TcpListener,
    session_builder: SessionBuilder,
    tls_config: Option<TlsAcceptor>,
}

impl SmtpServer {
    pub(crate) async fn new(conf: &Config) -> Result<Self, Error> {
        Ok(SmtpServer {
            tcp_listener: TcpListener::bind(conf.local_addr).await?,
            session_builder: SessionBuilder::new("TCP mail saver"),
            tls_config: conf.tls_config.clone().map(TlsAcceptor::from),
        })
    }

    pub(crate) async fn recv_mail(&self) -> Result<SmtpEmail, Error> {
        let (tcp_stream, peer_addr) = self.tcp_listener.accept().await?;
        info!("Accepted incoming TCP connection.");

        if let Some(acceptor) = &self.tls_config {
            self.handle_mail_comm(
                peer_addr,
                BufStream::new(acceptor.accept(tcp_stream).await?),
            )
            .await
        } else {
            self.handle_mail_comm(peer_addr, BufStream::new(tcp_stream))
                .await
        }
    }

    async fn handle_mail_comm(
        &self,
        peer_addr: SocketAddr,
        mut stream: impl AsyncBufReadExt + AsyncWriteExt + Unpin,
    ) -> Result<SmtpEmail, Error> {
        let (sender, receiver) = channel();
        let mut session = self
            .session_builder
            .build(peer_addr.ip(), MailHandler::new(sender));

        write_resp_async(session.greeting(), &mut stream).await?;
        stream.flush().await?;
        let mut ongoing_communication = true;
        while ongoing_communication {
            let mut line = String::new();
            stream.read_line(&mut line).await?;
            let resp = session.process(line.as_bytes());
            ongoing_communication = resp.action != response::Action::Close;
            write_resp_async(resp, &mut stream).await?;
            stream.flush().await?;
        }
        stream.shutdown().await?;

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

    fn data(&mut self, buf: &[u8]) -> std::io::Result<()> {
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

async fn write_resp_async(
    resp: mailin::response::Response,
    mut writer: impl AsyncWriteExt + Unpin,
) -> Result<(), Error> {
    // Store response in buffer:
    let mut buf = Vec::new();
    resp.write_to(&mut buf)?;

    // Write buffer asynchroniously:
    writer.write_all(buf.as_slice()).await?;

    Ok(())
}
