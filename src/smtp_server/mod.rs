use lettre::EmailAddress;
use log::{debug, error, warn};
use mailin::{response, Handler, Response, SessionBuilder};
use rustls::ServerConfig;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufStream},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::TlsAcceptor;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::{email::SmtpEmail, Error};

#[cfg(test)]
mod tests;

pub(crate) struct SmtpServer {
    tcp_listener: TcpListener,
    session_builder: SessionBuilder,
    tls_config: Option<TlsAcceptor>,
    implicit_tls: bool,
}

impl<'a> SmtpServer {
    pub(crate) async fn new(
        addr: &SocketAddr,
        tls_config: Option<Arc<ServerConfig>>,
    ) -> Result<Self, Error> {
        let mut smtp_session_builder = SessionBuilder::new("TCP mail saver");
        if tls_config.is_some() && addr.port() != 465 {
            smtp_session_builder.enable_start_tls();
        }
        let implicit_tls = tls_config.is_some() && addr.port() == 465;
        Ok(SmtpServer {
            tcp_listener: TcpListener::bind(addr).await?,
            session_builder: smtp_session_builder,
            tls_config: tls_config.map(TlsAcceptor::from),
            implicit_tls,
        })
    }

    pub(crate) async fn accept_conn(&self) -> Result<(TcpStream, SocketAddr), Error> {
        Ok(self.tcp_listener.accept().await?)
    }

    pub(crate) async fn recv_mail(
        &self,
        tcp_stream: TcpStream,
        peer_addr: SocketAddr,
        buf: &'a mut Vec<u8>,
    ) -> Result<SmtpEmail<'a>, Error> {
        if self.implicit_tls {
            self.handle_mail_comm(
                peer_addr,
                BufStream::new(
                    self.tls_config
                        .as_ref()
                        .expect("implicit_tls was true, but there was no TLS config.")
                        .accept(tcp_stream)
                        .await?,
                ),
                buf,
            )
            .await
        } else {
            self.handle_mail_comm(peer_addr, BufStream::new(tcp_stream), buf)
                .await
        }
    }

    async fn handle_mail_comm(
        &self,
        peer_addr: SocketAddr,
        mut stream: impl AsyncBufReadExt + AsyncWriteExt + Unpin,
        buf: &'a mut Vec<u8>,
    ) -> Result<SmtpEmail<'a>, Error> {
        let mut res = Err(Error::Smtp("No DATA_END reveived.".to_string()));
        let mail_handler = MailHandler::new(buf, &mut res);
        let mut session = self.session_builder.build(peer_addr.ip(), mail_handler);

        let greeting = session.greeting();
        write_resp_async(&greeting, &mut stream).await?;
        stream.flush().await?;
        let mut last_response = greeting;
        while last_response.action != response::Action::Close
            && last_response.action != response::Action::UpgradeTls
        {
            let mut line = String::new();
            stream.read_line(&mut line).await?;
            last_response = session.process(line.as_bytes());
            write_resp_async(&last_response, &mut stream).await?;
            stream.flush().await?;
        }
        // If the client requests TLS we upgrade the connection and go on as we would have with a TCP stream:
        if last_response.action == response::Action::UpgradeTls {
            let mut tls_stream = BufStream::new(
                self.tls_config
                    .as_ref()
                    .expect("STARTTLS was active, but there was no TLS config.")
                    .accept(stream)
                    .await?,
            );
            while last_response.action != response::Action::Close {
                let mut line = String::new();
                tls_stream.read_line(&mut line).await?;
                last_response = session.process(line.as_bytes());
                write_resp_async(&last_response, &mut tls_stream).await?;
                tls_stream.flush().await?;
            }
            tls_stream.shutdown().await?;
        } else {
            stream.shutdown().await?;
        }

        res
    }
}

struct MailHandler<'a, 'b> {
    from: Option<EmailAddress>,
    to: Vec<EmailAddress>,
    msg_buf: Option<&'a mut Vec<u8>>,
    received_mail: &'b mut Result<SmtpEmail<'a>, Error>,
}

impl<'a, 'b> MailHandler<'a, 'b> {
    fn new(
        buf: &'a mut Vec<u8>,
        result_pointer: &'b mut Result<SmtpEmail<'a>, Error>,
    ) -> MailHandler<'a, 'b> {
        MailHandler {
            from: None,
            to: vec![],
            msg_buf: Some(buf),
            received_mail: result_pointer,
        }
    }
}

impl<'a, 'b> Handler for MailHandler<'a, 'b> {
    fn helo(&mut self, _ip: IpAddr, _domain: &str) -> Response {
        response::OK
    }

    fn mail(&mut self, _ip: IpAddr, _domain: &str, from: &str) -> Response {
        match EmailAddress::new(String::from(from)) {
            Ok(m) => {
                self.from = Some(m);
                response::OK
            }
            Err(e) => {
                warn!("Incoming SMTP connection with invalid FROM mailbox: {}", e);
                response::BAD_MAILBOX
            }
        }
    }

    fn rcpt(&mut self, to: &str) -> Response {
        match EmailAddress::new(String::from(to)) {
            Ok(m) => {
                self.to.push(m);
                response::OK
            }
            Err(e) => {
                warn!("Incoming SMTP connection with invalid FROM mailbox: {}", e);
                response::BAD_MAILBOX
            }
        }
    }

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Response {
        debug!(
            "SMTP server eceived DATA_START: domain: {}, from: {}, 8bit: {}",
            _domain, _from, _is8bit
        );
        if self.msg_buf.is_none() {
            warn!("Received DATA_START after the message buf was taken.");
            return response::Response::custom(503, "Bad sequence of commands".to_string());
        } else if !self
            .msg_buf
            .as_ref()
            .expect("We checked this with the previous case.")
            .is_empty()
        {
            warn!("Received DATA_START while the message buf wasn't empty.");
            self.msg_buf
                .as_mut()
                .expect("We checked this with the previous case.")
                .clear();
        }
        response::OK
    }

    fn data(&mut self, buf: &[u8]) -> std::io::Result<()> {
        if let Some(ref mut buf_ref) = self.msg_buf {
            buf_ref.extend_from_slice(buf);
        } else {
            warn!("Received DATA_START after the message buf was taken.");
        }
        Ok(())
    }

    fn data_end(&mut self) -> Response {
        let buf_ref: &'a mut Vec<u8> = self.msg_buf.take().unwrap();
        let complete_mail = SmtpEmail::new(
            self.from.take(),
            self.to.drain(0..).collect(),
            buf_ref.as_slice(),
        );
        debug!("Received an email over SMTP.");
        match &self.received_mail {
            Err(Error::Smtp(_)) => {
                *self.received_mail = complete_mail;
                response::OK
            }
            Ok(_) => {
                error!("Reveiced DATA_END twice.");
                *self.received_mail = Err(Error::Smtp("Received multiple DATA_END.".to_string()));
                response::Response::custom(503, "Received multiple DATA_END.".to_string())
            }
            Err(_) => {
                error!("Reveiced DATA_END after previous error.");
                response::Response::custom(
                    554,
                    "Received DATA_END after previous error.".to_string(),
                )
            }
        }
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
    resp: &mailin::response::Response,
    mut writer: impl AsyncWriteExt + Unpin,
) -> Result<(), Error> {
    // Store response in buffer:
    let mut buf = Vec::new();
    resp.write_to(&mut buf)?;

    // Write buffer asynchroniously:
    writer.write_all(buf.as_slice()).await?;

    Ok(())
}
