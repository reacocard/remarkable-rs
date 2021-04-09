use std::fs;
use std::io;
use std::path;

use uuid::Uuid;
use zip::ZipArchive;

use crate::documents::{Document, Documents};

use crate::error::{Error, Result};

#[derive(serde::Serialize, serde::Deserialize, Default, Debug)]
pub struct ClientState {
    pub device_token: String,
    pub user_token: String,
    pub endpoint: String,
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
const QUERY_STORAGE_URL: &str = "https://service-manager-production-dot-remarkable-production.appspot.com/service/json/1/document-storage?environment=production&group=auth0|5a68dc51cb30df3877a1d7c4&apiVer=2";
const DOCUMENT_LIST_PATH: &str = "document-storage/json/2/docs";

#[derive(Debug)]
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

    pub async fn refresh_state(&mut self) -> Result<()> {
        self.refresh_token().await?;
        self.refresh_storage_endpoint().await?;
        Ok(())
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

    pub async fn refresh_storage_endpoint(&mut self) -> Result<()> {
        #[derive(Debug, serde::Deserialize)]
        struct StorageHost {
            #[serde(rename = "Status")]
            status: String,
            #[serde(rename = "Host")]
            host: String,
        }

        let response = self.http_client.get(QUERY_STORAGE_URL).send().await?;
        let storage_host: StorageHost =
            serde_json::from_str(&response.text().await?)?;

        if &storage_host.status != "OK" {
            eprintln!("Bad response from rM {:?}", storage_host);
            return Err(Error::RmCloudError);
        }
        self.client_state.endpoint = format!("https://{}", storage_host.host);
        Ok(())
    }

    fn get_document_list_url(&self) -> String {
        format!("{}/{}", self.client_state.endpoint, DOCUMENT_LIST_PATH)
    }

    pub async fn all_documents(&self, with_blob: bool) -> Result<Documents> {
        let mut request = self
            .http_client
            .get(&self.get_document_list_url())
            .bearer_auth(&self.client_state.user_token);

        if with_blob {
            request = request.query(&[("withBlob", "1")])
        }

        let response = request.send().await?;
        let body = response.text().await?;
        let docs = serde_json::from_str::<Documents>(&body)?;
        Ok(docs)
    }

    pub async fn download_zip(
        &self,
        id: Uuid,
    ) -> Result<ZipArchive<io::Cursor<bytes::Bytes>>> {
        let doc = self.get_document_by_id(id).await?;
        let response = self.http_client.get(doc.blob_url_get).send().await?;
        let bytes = response.bytes().await?;
        let seekable_bytes = io::Cursor::new(bytes); // ZipArchive wants something that is 'Seek'
        let zip = ZipArchive::new(seekable_bytes)?;
        Ok(zip)
    }

    pub async fn get_document_by_id(&self, id: Uuid) -> Result<Document> {
        let request = self
            .http_client
            .get(&self.get_document_list_url())
            .bearer_auth(&self.client_state.user_token)
            .query(&[("withBlob", "1"), ("doc", &id.to_string())]);
        let response = request.send().await?;
        let body = response.text().await?;
        let mut docs = serde_json::from_str::<Documents>(&body)?;
        match docs.remove(&id) {
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
