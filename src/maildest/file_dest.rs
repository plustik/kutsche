use std::path::PathBuf;

use async_trait::async_trait;
use log::info;
use tokio::{
    fs::OpenOptions,
    io::{AsyncWriteExt, BufWriter},
};

use super::EmailDestination;
use crate::email::Email;
use crate::Error;

pub(crate) struct FileDestination {
    base_path: PathBuf,
}

impl FileDestination {
    pub fn new<A: Into<PathBuf>>(path: A) -> Result<Self, Error> {
        let base_path = path.into();
        if base_path.is_dir() {
            Ok(Self { base_path })
        } else {
            Err(Error::SysIo(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "{} is not a directory.",
                    base_path.to_str().unwrap_or("The given path")
                ),
            )))
        }
    }
}

#[async_trait]
impl EmailDestination for FileDestination {
    async fn write_email(&self, email: &Email<'_>) -> Result<(), Error> {
        let mut dest_path = self.base_path.clone();
        dest_path.push(&email.message_id);
        let mut file_options = OpenOptions::new();
        file_options.write(true).create_new(true);
        let file = file_options.open(dest_path).await?;

        // Write email to file:
        let mut writer = BufWriter::new(file);
        // Write message ID:
        writer.write_all(email.message_id.as_bytes()).await?;
        writer.write_all("\n\n".as_bytes()).await?;
        // Write content:
        writer.write_all(email.raw).await?;

        writer.flush().await?;

        info!("Wrote email with id {} to filesystem.", &email.message_id);

        Ok(())
    }
}
