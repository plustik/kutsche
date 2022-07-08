use async_trait::async_trait;

use crate::email::Email;
use crate::Error;

mod file_dest;
mod matrix_dest;

pub(crate) use file_dest::FileDestination;
pub(crate) use matrix_dest::MatrixDestBuilder;

#[async_trait]
pub(crate) trait EmailDestination {
    async fn write_email(&self, email: &Email<'_>) -> Result<(), Error>;
}
