use std::io;
use std::result;

use derive_more::{Display, Error, From};

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, Display, Error, From)]
pub enum Error {
    InvalidZip,
    EmptyResult,
    RmCloudError,
    UuidError { source: uuid::Error },
    ZipError { source: zip::result::ZipError },
    IoError { source: io::Error },
    HttpError { source: reqwest::Error },
    JsonError { source: serde_json::Error },
}
