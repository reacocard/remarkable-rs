use std::fs;
use std::io;
use std::path;

use uuid::Uuid;

use crate::documents::{Document, Documents};

use crate::error::{Error, Result};

#[derive(serde::Serialize, serde::Deserialize, Default, Debug)]
pub struct ClientState {
    device_token: String,
    user_token: String,
    endpoint: String,
}

impl ClientState {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn load<R>(&mut self, f: R) -> Result<()>
    where
        R: io::Read,
    {
        #[allow(clippy::unit_arg)]
        Ok(*self = serde_json::from_reader(f)?)
    }

    pub fn load_from_path(&mut self, p: &path::Path) -> Result<()> {
        Ok(self.load(io::BufReader::new(fs::File::open(p)?))?)
    }

    pub fn save<W>(&self, f: W) -> Result<()>
    where
        W: io::Write,
    {
        Ok(serde_json::to_writer_pretty(f, self)?)
    }

    pub fn save_to_path(self, p: &path::Path) -> Result<()> {
        // TODO: Make this be properly atomic
        Ok(self.save(io::BufWriter::new(fs::File::create(p)?))?)
    }
}

const USER_TOKEN_URL: &str = "https://my.remarkable.com/token/json/2/user/new";
const DOCUMENT_LIST_PATH: &str = "document-storage/json/2/docs";

pub struct Client {
    client_state: ClientState,
    http_client: reqwest::Client,
}

impl Client {
    pub fn new(
        client_state: ClientState,
        http_client: reqwest::Client,
    ) -> Self {
        Client {
            client_state,
            http_client,
        }
    }

    pub fn state(&mut self) -> &mut ClientState {
        &mut self.client_state
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub async fn refresh_token(&mut self) -> Result<()> {
        let request = self
            .http_client
            .post(USER_TOKEN_URL)
            .bearer_auth(&self.client_state.device_token)
            .body("")
            .header(reqwest::header::CONTENT_LENGTH, "0");
        let response = request.send().await?;
        self.client_state.user_token = response.text().await?;
        Ok(())
    }

    fn get_document_list_url(&self) -> String {
        format!("{}/{}", self.client_state.endpoint, DOCUMENT_LIST_PATH)
    }

    pub async fn get_documents(&self) -> Result<Documents> {
        let request = self
            .http_client
            .get(&self.get_document_list_url())
            .bearer_auth(&self.client_state.user_token);
        let response = request.send().await?;
        let body = response.text().await?;
        let docs = serde_json::from_str::<Documents>(&body)?;
        Ok(docs)
    }

    pub async fn get_document_by_id(&self, id: &Uuid) -> Result<Document> {
        let request = self
            .http_client
            .get(&self.get_document_list_url())
            .bearer_auth(&self.client_state.user_token)
            .query(&[("withBlob", "1"), ("doc", &id.to_string())]);
        let response = request.send().await?;
        let body = response.text().await?;
        let mut docs = serde_json::from_str::<Documents>(&body)?;
        match docs.remove(id) {
            Some(d) => Ok(d),
            None => Err(Error::EmptyResult),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
