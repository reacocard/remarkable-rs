use std::fs;
use std::io;
use std::path;

use uuid::Uuid;
use zip::ZipArchive;

use crate::documents::{Document, Documents, Parent, UploadDocument};

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
const UPLOAD_PATH: &str = "document-storage/json/2/upload/request";
const UPDATE_STATUS_PATH: &str = "document-storage/json/2/upload/update-status";

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

    fn document_list_url(&self) -> String {
        format!("{}/{}", self.client_state.endpoint, DOCUMENT_LIST_PATH)
    }

    fn upload_url(&self) -> String {
        format!("{}/{}", self.client_state.endpoint, UPLOAD_PATH)
    }

    fn update_status_url(&self) -> String {
        format!("{}/{}", self.client_state.endpoint, UPDATE_STATUS_PATH)
    }

    pub async fn all_documents(&self, with_blob: bool) -> Result<Documents> {
        let mut request = self
            .http_client
            .get(&self.document_list_url())
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
            .get(&self.document_list_url())
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

    fn prepare_empty_zip_content(id: Uuid) -> Result<Vec<u8>> {
        use std::io::Write;

        let buf = Vec::new();
        let w = std::io::Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(w);

        let options = zip::write::FileOptions::default();
        zip.start_file(format!("{}.content", id), options)?;
        zip.write(b"{}")?;

        // Optionally finish the zip. (this is also done on drop)
        let writer = zip.finish()?;
        Ok(writer.into_inner())
    }

    pub async fn create_folder(
        &self,
        visible_name: String,
        parent: Parent,
    ) -> Result<Uuid> {
        println!("Creating folder {} {:?}", visible_name, parent);

        println!("Sending upload_request {:?}", upload_req);

        let mut folder_doc = UploadDocument::new_folder(visible_name, parent);
        let upload_req = &[folder_doc.upload_request()];

        let raw_upload_req_response = self
            .http_client
            .put(self.upload_url())
            .bearer_auth(&self.client_state.user_token)
            .json(upload_req)
            .send()
            .await?;

        println!("Received upload req response {:?}", raw_upload_req_response);

        #[derive(Debug, serde::Deserialize)]
        struct UploadRequestResponse {
            #[serde(rename = "ID")]
            id: Uuid,
            #[serde(rename = "Version")]
            version: u32,
            #[serde(rename = "Message")]
            message: String,
            #[serde(rename = "Success")]
            success: bool,
            #[serde(rename = "BlobURLPut")]
            blob_url_put: String,
            #[serde(rename = "BlobURLPutExpires")]
            blob_url_put_expires: String,
        }

        let mut upload_req_responses: Vec<UploadRequestResponse> =
            serde_json::from_str(&raw_upload_req_response.text().await?)?;

        println!("Response from rM {:?}", upload_req_responses);
        let upload_req_response = match upload_req_responses.pop() {
            Some(response) => response,
            None => {
                eprintln!(
                    "Did not receive a valid upload request response from rM Cloud {:?}",
                    upload_req_responses
                );
                return Err(Error::RmCloudError);
            }
        };

        if !upload_req_response.success {
            eprintln!(
                "Bad response from rM when creating upload request {:?}",
                upload_req_response
            );
            return Err(Error::RmCloudError);
        }

        // Update the our folder id, just in case rM wants us to use a different ID from the one we requested
        folder_doc.id = upload_req_response.id;
        let zip_content = Self::prepare_empty_zip_content(folder_doc.id)?;

        let raw_upload_response = self
            .http_client
            .put(upload_req_response.blob_url_put)
            .bearer_auth(&self.client_state.user_token)
            .header("Content-Type", "")
            .body(zip_content)
            .send()
            .await?;

        if raw_upload_response.status() != 200 {
            eprintln!(
                "Bad response from rM when upload folder {:?}",
                raw_upload_response
            );
            return Err(Error::RmCloudError);
        }

        let raw_update_status_response = self
            .http_client
            .put(self.update_status_url())
            .bearer_auth(&self.client_state.user_token)
            .json(&[folder_doc])
            .send()
            .await?;

        #[derive(Debug, serde::Deserialize)]
        struct UpdateStatusResponse {
            #[serde(rename = "ID")]
            id: Uuid,
            #[serde(rename = "Version")]
            version: u32,
            #[serde(rename = "Message")]
            message: String,
            #[serde(rename = "Success")]
            success: bool,
        }
        let mut update_status_responses: Vec<UpdateStatusResponse> =
            serde_json::from_str(&raw_update_status_response.text().await?)?;

        if update_status_responses.len() != 1 {
            eprintln!(
                "Expecte a singel response for our update_status request, got {:?}",
                update_status_responses
            );
        }
        let update_status = update_status_responses.pop().unwrap();
        println!("Got update status {:?}", update_status);
        if !update_status.success {
            eprintln!("Failed to update status of folder {:?}", update_status);
            Err(Error::RmCloudError)
        } else {
            Ok(update_status.id)
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
