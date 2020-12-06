use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path;
use std::result;

use derive_more::{Display, Error, From};
use serde::de::Deserialize;
use uuid::Uuid;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, Display, Error, From)]
pub enum Error {
    EmptyResult,
    IoError { source: io::Error },
    HttpError { source: reqwest::Error },
    JsonError { source: serde_json::Error },
}

#[derive(serde::Deserialize, Debug)]
pub struct Document {
    // The serde renames are to map rust-style names to the JSON api.
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "VissibleName")]
    pub visible_name: String,
    #[serde(rename = "Parent", deserialize_with = "deserialize_optional_uuid")]
    pub parent: Option<Uuid>,
    #[serde(rename = "Type")]
    pub doc_type: String,
    #[serde(rename = "CurrentPage")]
    pub current_page: i32,
    #[serde(rename = "Bookmarked")]
    pub bookmarked: bool,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "ModifiedClient")]
    pub modified_client: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "BlobURLGet")]
    pub blob_url_get: String,
    #[serde(rename = "BlobURLGetExpires")]
    pub blob_url_get_expires: chrono::DateTime<chrono::Utc>,
}

// Extends UUID parsing by representing empty string as None
fn deserialize_optional_uuid<'de, D>(deserializer: D) -> result::Result<Option<Uuid>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;

    if buf == "" {
        Ok(None)
    } else {
        Uuid::parse_str(&buf)
            .map(Some)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Default)]
pub struct Documents {
    by_id: HashMap<Uuid, Document>,
}

impl Documents {
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, uuid: &Uuid) -> Option<&Document> {
        self.by_id.get(uuid)
    }

    pub fn get_by_path(&self, path: &path::Path) -> Option<&Document> {
        // TODO: The runtime of this is O(n^m) where n is the total number of
        // documents and m is the number of path components. Since we have O(1)
        // lookup by id this should be doable in O(n).
        for d in self.by_id.values() {
            if d.visible_name
                == path
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
            {
                match path.parent().zip(d.parent) {
                    None => return Some(d),
                    Some((parent_path, parent_id)) => match self.get_by_path(parent_path) {
                        None => continue,
                        Some(parent) => {
                            if parent.id == parent_id {
                                return Some(d);
                            }
                        }
                    },
                }
            }
        }
        None
    }

    pub fn get_children(&self, uuid: &Option<Uuid>) -> Vec<&Document> {
        let mut acc: Vec<&Document> = vec![];
        for d in self.by_id.values() {
            if d.parent == *uuid {
                acc.push(&d);
            }
        }
        acc
    }
}

impl<'de> serde::de::Deserialize<'de> for Documents {
    fn deserialize<D>(deserializer: D) -> result::Result<Documents, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DocumentsVisitor;

        impl<'de> serde::de::Visitor<'de> for DocumentsVisitor {
            type Value = Documents;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON sequence")
            }

            fn visit_seq<V>(self, mut visitor: V) -> result::Result<Self::Value, V::Error>
            where
                V: serde::de::SeqAccess<'de>,
            {
                let mut documents: Documents = Default::default();

                while let Some(doc) = visitor.next_element::<Document>()? {
                    documents.by_id.insert(doc.id, doc);
                }

                Ok(documents)
            }
        }

        deserializer.deserialize_any(DocumentsVisitor)
    }
}

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
    pub fn new(client_state: ClientState, http_client: reqwest::Client) -> Self {
        Client {
            client_state,
            http_client,
        }
    }

    pub fn state(&mut self) -> &mut ClientState {
        &mut self.client_state
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
        match docs.by_id.remove(id) {
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
