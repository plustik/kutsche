use mailin::{response, Handler, Response, SessionBuilder};

use std::io::{self, BufRead, BufReader, BufWriter};
use std::net::{IpAddr, TcpListener};

use crate::Error;

pub(crate) struct SmtpServer {
    tcp_listener: TcpListener,
    session_builder: SessionBuilder,
}

impl SmtpServer {
    pub(crate) fn new() -> Result<Self, Error> {
        Ok(SmtpServer {
            tcp_listener: TcpListener::bind("127.0.0.1:25")?,
            session_builder: SessionBuilder::new("TCP mail saver"),
        })
    }

    pub(crate) fn recv_mail(&self) -> Result<Vec<u8>, Error> {
        let (stream, peer_addr) = self.tcp_listener.accept()?;
        let mut conn_buf_read = BufReader::new(&stream);
        let mut conn_buf_write = BufWriter::new(&stream);

        let mut session = self
            .session_builder
            .build(peer_addr.ip(), MailHandler::new());

        session.greeting().write_to(&mut conn_buf_write)?;
        let mut buf = String::new();
        let mut ongoing_communication = true;
        while ongoing_communication {
            buf.clear();
            conn_buf_read.read_line(&mut buf)?;
            let resp = session.process(buf.as_bytes());
            ongoing_communication = resp.action == response::Action::Close;
            resp.write_to(&mut conn_buf_write)?;
        }

        Ok(vec![])
    }
}

struct MailHandler {
    buf: Option<Vec<u8>>,
}

impl MailHandler {
    fn new() -> Self {
        MailHandler { buf: None }
    }
}

impl Handler for MailHandler {
    fn helo(&mut self, ip: IpAddr, domain: &str) -> Response {
        println!("Received SMTP helo: {}, {}", ip, domain);
        response::OK
    }

    fn mail(&mut self, _ip: IpAddr, _domain: &str, from: &str) -> Response {
        println!("Mail message started: {}", from);
        response::OK
    }

    fn rcpt(&mut self, to: &str) -> Response {
        println!("Mail recipient set: {}", to);
        response::OK
    }

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Response {
        self.buf = Some(vec![]);
        response::OK
    }

    fn data(&mut self, buf: &[u8]) -> io::Result<()> {
        self.buf
            .as_mut()
            .expect("Received data but no data command.")
            .extend_from_slice(buf);
        Ok(())
    }

    fn data_end(&mut self) -> Response {
        println!(
            "All mail data received:\n{}",
            String::from_utf8(
                self.buf
                    .take()
                    .expect("Received data end command but no data start command.")
            )
            .expect("Email data is invalid UTF-8.")
        );
        response::OK
    }
}
