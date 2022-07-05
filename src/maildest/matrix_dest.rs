use log::{error, info};
use matrix_sdk::{Client, ClientBuildError};

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use super::EmailDestination;
use crate::email::Email;
use crate::Error;

pub(crate) struct MatrixDestBuilder<'a> {
    matrix_client: Client,
    session_file_path: Option<&'a Path>,
    login_data: Option<(&'a str, &'a str)>, // username, password
}
impl<'a> MatrixDestBuilder<'a> {
    pub async fn new(homeserver_url: impl AsRef<str>) -> Result<MatrixDestBuilder<'a>, Error> {
        let matrix_client = match Client::builder()
            .homeserver_url(homeserver_url)
            .respect_login_well_known(true)
            .build()
            .await
        {
            Ok(c) => c,
            Err(ClientBuildError::Url(url_parse_err)) => {
                return Err(Error::Config(format!(
                    "Could not parse homeserver URL: {}",
                    url_parse_err
                )));
            }
            Err(ClientBuildError::Http(http_err)) => {
                return Err(Error::Matrix(format!(
                    "Error during HTTP request: {}",
                    http_err
                )));
            }
            Err(ClientBuildError::AutoDiscovery(err)) => {
                return Err(Error::Matrix(format!(
                    "Could not perform auto-discovery: {}",
                    err
                )));
            }
            Err(ClientBuildError::SledStore(_)) => {
                error!("Creation of matrix client resulted in unexpected sled error.");
                panic!("I don't think this can happen, because the default memory store does not use sled.");
            }
            Err(ClientBuildError::MissingHomeserver) => {
                error!("Creation of matrix client resulted in unexpected MissingHomeserver error.");
                panic!(
                    "This shouldn't be possible, because we called .homeserver_url() previously."
                );
            }
        };

        Ok(MatrixDestBuilder {
            matrix_client,
            session_file_path: None,
            login_data: None,
        })
    }

    pub fn set_login(&mut self, user: &'a str, password: &'a str) {
        self.login_data = Some((user, password));
    }

    pub fn set_session_path(&mut self, session_file_path: &'a Path) {
        self.session_file_path = Some(session_file_path);
    }

    /// Creates a new MatrixDestination by logging the internal Matrix client in or restoring an existing session.
    ///
    /// If an existing file was set with `set_session_path()` a session is restored from this file.
    /// Otherwise, if login data was set with `set_login()` a new session is created. If a non-existing session file was set with
    /// `set_session_path()` the new session is saved to the given path.
    /// If neither an existing session file nor login data is given, an error is returned.
    pub async fn build(self) -> Result<MatrixDestination, Error> {
        // We allow blocking calls in this function, because it should only be called during the startup of the server.

        if self.session_file_path.is_some()
            && self
                .session_file_path
                .expect("We call .is_some() in the same line")
                .is_file()
        {
            let session_file = File::open(
                self.session_file_path
                    .expect("We call .is_some() in the if-clause."),
            )?;
            let session = serde_json::from_reader(BufReader::new(session_file))
                .map_err(|e| Error::Config(format!("Could not parse session file: {}", e)))?;
            self.matrix_client.restore_login(session).await?;
        } else {
            let (username, password) = self.login_data.ok_or_else(|| {
                Error::Config("Missing session file path or login data.".to_string())
            })?;
            self.matrix_client
                .login(username, password, None, Some("kutsche-server"))
                .await?;
            // If a nonexisting session file is given, we create is and save the new session:
            if self.session_file_path.is_some() {
                let session_file = File::create(
                    self.session_file_path
                        .expect("We called .is_some() in the if-clause."),
                )?;
                serde_json::to_writer_pretty(
                    BufWriter::new(session_file),
                    &self
                        .matrix_client
                        .session()
                        .await
                        .expect("We only call this after logging in previously."),
                )
                .map_err(|e| Error::Config(format!("Could save session to file: {}", e)))?;
            }
        }
        if !self.matrix_client.logged_in().await {
            error!("Tried to use a matrix client, that was not logged in.");
            panic!("Called MatrixDestBuilder.build() before logging in or restoring a session.");
        }

        Ok(MatrixDestination {
            matrix_client: self.matrix_client,
        })
    }
}

pub(crate) struct MatrixDestination {
    matrix_client: Client,
}

impl EmailDestination for MatrixDestination {
    fn write_email(&self, email: &Email) -> Result<(), Error> {
        info!("Wrote email with id {} to filesystem.", &email.message_id);
        todo!();
    }
}
