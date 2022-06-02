use std::{env::args, io};

use log::{info, LevelFilter};
use log4rs::{
    append::console::ConsoleAppender,
    config::{Appender, Config, Root},
};
use users::switch::{set_effective_gid, set_effective_uid};

use smtp_server::SmtpServer;

mod config;
mod email;
mod maildest;
mod smtp_server;

#[tokio::main]
async fn main() {
    let config = config::Config::with_args(
        args().skip_while(|s| s.ends_with("kutsche") && !s.starts_with('-')),
    )
    .expect("Could not parse configuration.");

    init_logger(&config).expect("Could not initialize logger.");

    let smtp_server = SmtpServer::new(&config)
        .await
        .expect("Could not bind to TcpSocket.");

    // Dropping privileges:
    if let Some(user) = config.effective_user {
        info!("Changing effective user ID to {}...", user.uid());
        set_effective_uid(user.uid()).expect("Could not change effective user.");
    }
    if let Some(group) = config.effective_group {
        info!("Changing effective group ID to {}...", group.gid());
        set_effective_gid(group.gid()).expect("Could not change effective group.");
    }
    info!("Dropped privileges.");

    info!("Accepting connections...");
    loop {
        let email = smtp_server
            .recv_mail()
            .await
            .expect("Could not receive email.");
        for addr in email.to {
            if let Some(dest) = config.dest_map.get(AsRef::<str>::as_ref(&addr)) {
                dest.write_email(&email.content)
                    .expect("Could not forward email.");
            }
        }
    }
}

fn init_logger(_conf: &config::Config) -> Result<(), Error> {
    let stdout = ConsoleAppender::builder().build();

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .build(Root::builder().appender("stdout").build(LevelFilter::Info))?;

    log4rs::init_config(config)?;

    Ok(())
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
impl From<log4rs::config::runtime::ConfigErrors> for Error {
    fn from(inner: log4rs::config::runtime::ConfigErrors) -> Self {
        match inner.errors().first() {
            Some(log4rs::config::runtime::ConfigError::DuplicateAppenderName(descr)) => {
                Self::Config(format!(
                    "Duplicate Appender name in logger configuration: {}",
                    descr
                ))
            }
            Some(log4rs::config::runtime::ConfigError::NonexistentAppender(descr)) => Self::Config(
                format!("Nonexistent Appender in logger configuration: {}", descr),
            ),
            Some(log4rs::config::runtime::ConfigError::DuplicateLoggerName(descr)) => Self::Config(
                format!("Duplicate Logger name in logger configuration: {}", descr),
            ),
            Some(log4rs::config::runtime::ConfigError::InvalidLoggerName(descr)) => Self::Config(
                format!("Invalid Logger name in logger configuration: {}", descr),
            ),
            _ => Self::Config("Error in logger configuration.".to_string()),
        }
    }
}
impl From<log::SetLoggerError> for Error {
    fn from(inner: log::SetLoggerError) -> Self {
        Self::Config(format!("Error while setting logger: {}", inner))
    }
}
