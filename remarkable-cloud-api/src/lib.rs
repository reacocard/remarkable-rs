pub use reqwest;

mod client;
pub use crate::client::{Client, ClientState};

mod documents;
pub use crate::documents::{Document, Documents, Parent, Uuid};

mod error;
pub use crate::error::{Error, Result};

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
