use std::{env::args, io};

use users::switch::{set_effective_gid, set_effective_uid};

use smtp_server::SmtpServer;

mod config;
mod email;
mod maildest;
mod smtp_server;

fn main() {
    let config = config::Config::with_args(
        args().skip_while(|s| s.ends_with("kutsche") && !s.starts_with('-')),
    )
    .expect("Could not parse configuration.");

    let smtp_server = SmtpServer::new(&config).expect("Could not bind to TcpSocket.");

    // Dropping privileges:
    if let Some(user) = config.effective_user {
        set_effective_uid(user.uid()).expect("Could not change effective user.");
    }
    if let Some(group) = config.effective_group {
        set_effective_gid(group.gid()).expect("Could not change effective group.");
    }

    loop {
        let email = smtp_server.recv_mail().expect("Could not receive email.");
        for addr in email.to {
            if let Some(dest) = config.dest_map.get(AsRef::<str>::as_ref(&addr)) {
                dest.write_email(&email.content)
                    .expect("Could not forward email.");
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    Config(String),
    General(String),
    NotADir,
    Parsing(&'static str),
    SysIo(io::Error),
    Tls(rustls::Error),
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
impl From<rustls::Error> for Error {
    fn from(inner: rustls::Error) -> Self {
        Self::Tls(inner)
    }
}
