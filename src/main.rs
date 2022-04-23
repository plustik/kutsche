use std::io;

use smtp_server::SmtpServer;

mod email;
mod smtp_server;

fn main() {
    let smpt_server = SmtpServer::new("127.0.0.1:25").expect("Could not bind to TcpSocket.");

    loop {
        smpt_server.recv_mail().expect("Could not receive mail.");
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    SysIo(io::Error),
    Parsing(&'static str),
}

impl From<io::Error> for Error {
    fn from(inner: io::Error) -> Self {
        Self::SysIo(inner)
    }
}
