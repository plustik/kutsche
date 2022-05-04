use std::io;

use maildest::{EmailDestination, FileDestination};
use smtp_server::SmtpServer;

mod email;
mod maildest;
mod smtp_server;

fn main() {
    const DEST_DIR: &str = "./received_mail";
    let file_dest =
        FileDestination::new(DEST_DIR).expect("The given destination directory does not exist.");

    let smpt_server = SmtpServer::new("127.0.0.1:25").expect("Could not bind to TcpSocket.");

    loop {
        file_dest
            .write_email(smpt_server.recv_mail().expect("Could not receive mail."))
            .expect("Could not write email to file.");
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    NotADir,
    Parsing(&'static str),
    SysIo(io::Error),
}

impl From<io::Error> for Error {
    fn from(inner: io::Error) -> Self {
        Self::SysIo(inner)
    }
}
