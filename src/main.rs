use std::{env::args, io};

use maildest::{EmailDestination, FileDestination};
use smtp_server::SmtpServer;

mod config;
mod email;
mod maildest;
mod smtp_server;

fn main() {
    let config = config::Config::with_args(args()).expect("Could not parse configuration.");

    const DEST_DIR: &str = "./received_mail";
    let file_dest =
        FileDestination::new(DEST_DIR).expect("The given destination directory does not exist.");

    let smpt_server = SmtpServer::new(config.local_addr).expect("Could not bind to TcpSocket.");

    loop {
        file_dest
            .write_email(smpt_server.recv_mail().expect("Could not receive mail."))
            .expect("Could not write email to file.");
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    Config(String),
    General(String),
    NotADir,
    Parsing(&'static str),
    SysIo(io::Error),
}

impl From<io::Error> for Error {
    fn from(inner: io::Error) -> Self {
        Self::SysIo(inner)
    }
}
impl From<String> for Error {
    fn from(inner: String) -> Self {
        Self::General(inner)
    }
}
