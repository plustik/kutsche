use crate::email::Email;
use crate::Error;

mod file_dest;
mod matrix_dest;

pub(crate) use file_dest::FileDestination;
pub(crate) use matrix_dest::MatrixDestBuilder;

pub(crate) trait EmailDestination {
    fn write_email(&self, email: &Email) -> Result<(), Error>;
}
