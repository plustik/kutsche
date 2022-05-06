use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::email::Email;
use crate::Error;

pub(crate) trait EmailDestination {
    fn write_email(&self, email: &Email) -> Result<(), Error>;
}

pub(crate) struct FileDestination {
    base_path: PathBuf,
}

impl FileDestination {
    pub fn new<A: Into<PathBuf>>(path: A) -> Result<Self, Error> {
        let base_path = path.into();
        if base_path.is_dir() {
            Ok(Self { base_path })
        } else {
            Err(Error::NotADir)
        }
    }
}

impl EmailDestination for FileDestination {
    fn write_email(&self, email: &Email) -> Result<(), Error> {
        if !self.base_path.is_dir() {
            return Err(Error::NotADir);
        }

        let mut dest_path = self.base_path.clone();
        dest_path.push(&email.message_id);
        let mut file_options = OpenOptions::new();
        file_options.write(true).create_new(true);
        let file = file_options.open(dest_path)?;

        // Write email to file:
        let mut writer = BufWriter::new(file);
        // Write message ID:
        writer.write_all(email.message_id.as_bytes())?;
        writer.write_all("\n\n".as_bytes())?;
        // Write content:
        writer.write_all(&email.data)?;

        writer.flush()?;

        Ok(())
    }
}
