use log::{error, info, warn, LevelFilter};
use log4rs::{
    append::console::ConsoleAppender,
    config::{Appender, Config, Root},
};
use users::switch::{set_effective_gid, set_effective_uid};

use std::{env::args, fmt, io, process::ExitCode, sync::Arc};

use smtp_server::SmtpServer;

mod config;
mod email;
mod maildest;
mod smtp_server;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match config::Config::with_args(
        args().skip_while(|s| s.ends_with("kutsche") && !s.starts_with('-')),
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error while loading configuration: {}", &e);
            error!("Could not load configuration: {}", e);
            return ExitCode::from(1);
        }
    };

    if let Err(e) = init_logger(&config) {
        eprintln!("Error while initializing logger: {}", &e);
        error!("Could not initialize logger: {}", e);
        return ExitCode::from(2);
    }

    // TODO: Refactor to filter_map when async closures become stable (issue 62290)
    let mut smtp_servers = Vec::new();
    for addr in config.local_addrs.iter() {
        match SmtpServer::new(addr, config.tls_config.clone()).await {
            Ok(server) => {
                log::info!("Startet server bound to {}", addr);
                smtp_servers.push(server);
            }
            Err(e) => {
                eprintln!(
                    "Error while starting server for local address {}: {}",
                    addr, &e
                );
                error!("Could not start server for local address {}: {}", addr, e);
            }
        }
    }
    if smtp_servers.is_empty() {
        eprintln!("Starting server failed for all local addresses.");
        error!("Could not start server for any local address.");
        return ExitCode::from(3);
    } else {
        info!("Started {} SMTP servers.", smtp_servers.len());
    }

    // Dropping privileges:
    if let Some(user) = &config.effective_user {
        info!("Changing effective user ID to {}...", user.uid());
        if let Err(e) = set_effective_uid(user.uid()) {
            eprintln!("Error while changing effective user: {}", &e);
            error!("Could not change effective user: {}", e);
            return ExitCode::from(4);
        }
    }
    if let Some(group) = &config.effective_group {
        info!("Changing effective group ID to {}...", group.gid());
        if let Err(e) = set_effective_gid(group.gid()) {
            eprintln!("Error while changing effective group: {}", &e);
            error!("Could not change effective group: {}", e);
            return ExitCode::from(5);
        }
    }
    if config.effective_user.is_some() || config.effective_group.is_some() {
        info!("Dropped privileges.");
    }

    info!("Accepting connections...");
    let config = Arc::new(config);
    for server in smtp_servers {
        let config_ref = config.clone();
        let server_ref = Arc::new(server);
        tokio::spawn(async move {
            loop {
                let (stream, addr) = match server_ref.accept_conn().await {
                    Err(e) => {
                        eprintln!("Error while accepting TCP connection: {}", &e);
                        error!("Could not accept TCP connection: {}", e);
                        continue;
                    }
                    Ok((stream, addr)) => {
                        info!("Accepted incoming TCP connection.");
                        (stream, addr)
                    }
                };
                let config = config_ref.clone();
                let server = server_ref.clone();
                tokio::spawn(async move {
                    match server.recv_mail(stream, addr).await {
                        Ok(email) => {
                            for addr in email.to {
                                if let Some(dest) = config.dest_map.get(AsRef::<str>::as_ref(&addr))
                                {
                                    if let Err(e) = dest.write_email(&email.content) {
                                        eprintln!("Error while forwarding email: {}", &e);
                                        error!("Could not forward email: {}", e);
                                    }
                                } else {
                                    warn!("Received an email without a destination mapping.");
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error while receiving email: {}", &e);
                            error!("Could not receive mail: {}", e);
                        }
                    }
                });
            }
        });
    }

    ExitCode::SUCCESS
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
    MailParsing(&'static str),
    Smtp(String),
    SysIo(io::Error),
    Tls(rustls::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;

        match self {
            Config(desc) => write!(f, "Error in config: {}", desc),
            MailParsing(desc) => write!(f, "Could not parse email: {}", desc),
            Smtp(desc) => write!(f, "Error in SMTP communication: {}", desc),
            SysIo(inner) => write!(f, "IO error: {}", inner),
            Tls(inner) => write!(f, "TLS error: {}", inner),
        }
    }
}

impl From<io::Error> for Error {
    fn from(inner: io::Error) -> Self {
        Self::SysIo(inner)
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
