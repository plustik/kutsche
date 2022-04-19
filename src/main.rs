use std::io;

use smtp_server::SmtpServer;

mod smtp_server;

fn main() {
    let smpt_server = SmtpServer::new().expect("Could not bind to TcpSocket.");

    loop {
        smpt_server.recv_mail().expect("Could not receive mail.");
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    SysIo(io::Error),
}

impl From<io::Error> for Error {
    fn from(inner: io::Error) -> Self {
        Self::SysIo(inner)
    }
}
