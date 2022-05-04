use std::ffi::OsStr;
use std::fs::{DirBuilder, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::email::SmtpEmail;
use crate::Error;

pub(crate) trait EmailDestination {
    fn write_email(&self, email: SmtpEmail) -> Result<(), Error>;
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
    fn write_email(&self, email: SmtpEmail) -> Result<(), Error> {
        assert_eq!(email.to.len(), 1);

        if !self.base_path.is_dir() {
            return Err(Error::NotADir);
        }

        let mut dest_path = self.base_path.clone();
        dest_path.push(AsRef::<OsStr>::as_ref(&email.to[0]));

        if !dest_path.is_dir() {
            DirBuilder::new().create(&dest_path)?;
        }

        dest_path.push(&email.message_id);
        let mut file_options = OpenOptions::new();
        file_options.write(true).create_new(true);
        let file = file_options.open(dest_path)?;

        // Write email to file:
        let mut writer = BufWriter::new(file);
        // Write message ID:
        writer.write_all(email.message_id.as_bytes())?;
        writer.write_all("\n".as_bytes())?;
        // Write from address:
        if let Some(addr) = email.from {
            writer.write_all(AsRef::<str>::as_ref(&addr).as_bytes())?;
            writer.write_all("\n".as_bytes())?;
        }
        writer.write_all("\n".as_bytes())?;
        // Write content:
        writer.write_all(&email.data)?;

        writer.flush()?;

        Ok(())
    }
}
