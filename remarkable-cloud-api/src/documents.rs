use std::collections::HashMap;
use std::fmt;
use std::path;
use std::result;

use serde::de::Deserialize;
pub use uuid::Uuid;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Parent {
    Root,
    Trash,
    Node(Uuid),
}

impl Parent {
    fn to_rm_string(&self) -> String {
        match self {
            Self::Root => "".to_string(),
            Self::Trash => "trash".to_string(),
            Self::Node(id) => format!("{}", id),
        }
    }

    fn serialize_rm_parent<S>(&self, se: S) -> result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        se.serialize_str(&self.to_rm_string())
    }

    fn deserialize_rm_parent<'de, D>(de: D) -> result::Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let buf = String::deserialize(de)?;

        match buf.as_ref() {
            "" => Ok(Self::Root),
            "trash" => Ok(Self::Trash),
            uuid => Uuid::parse_str(uuid)
                .map(Self::Node)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct UploadRequest {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "Type")]
    pub doc_type: String,
    #[serde(rename = "Version")]
    pub version: u32,
}

impl UploadRequest {
    pub fn new_notebook(id: Uuid) -> Self {
        Self {
            id,
            doc_type: "DocumentType".into(),
            version: 1,
        }
    }

    pub fn new_folder(id: Uuid) -> Self {
        Self {
            id,
            doc_type: "CollectionType".into(),
            version: 1,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct UploadRequestResponse {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "Version")]
    pub version: u32,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Success")]
    pub success: bool,
    #[serde(rename = "BlobURLPut")]
    pub blob_url_put: String,
    #[serde(rename = "BlobURLPutExpires")]
    pub blob_url_put_expires: String,
}

#[derive(Debug, serde::Serialize)]
pub struct UpdateStatusRequest {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "Parent", serialize_with = "Parent::serialize_rm_parent")]
    pub parent: Parent,
    #[serde(rename = "VissibleName")]
    pub visible_name: String,
    #[serde(rename = "Type")]
    pub doc_type: String,
    #[serde(rename = "Version")]
    pub version: u32,
    #[serde(rename = "ModifiedClient")]
    pub modified_client: chrono::DateTime<chrono::Utc>,
}

impl UpdateStatusRequest {
    pub fn after_upload(
        upload_request: UploadRequest,
        upload_request_response: UploadRequestResponse,
        visible_name: String,
        parent: Parent,
    ) -> Self {
        Self {
            id: upload_request_response.id,
            parent,
            visible_name,
            doc_type: upload_request.doc_type,
            version: upload_request_response.version,
            modified_client: chrono::Utc::now(),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateStatusResponse {
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "Version")]
    pub version: u32,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Success")]
    pub success: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct Document {
    // The serde renames are to map rust-style names to the JSON api.
    #[serde(rename = "ID")]
    pub id: Uuid,
    #[serde(rename = "VissibleName")]
    pub visible_name: String,
    #[serde(
        rename = "Parent",
        deserialize_with = "Parent::deserialize_rm_parent"
    )]
    pub parent: Parent,
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

#[derive(Debug, Default)]
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

    pub fn iter(&self) -> impl Iterator<Item = &Document> {
        self.by_id.values()
    }

    pub fn get(&self, uuid: &Uuid) -> Option<&Document> {
        self.by_id.get(uuid)
    }

    pub fn get_by_path(&self, path: &path::Path) -> Option<&Document> {
        // TODO: The runtime of this is O(n^m) where n is the total number of
        // documents and m is the number of path components. Since we have O(1)
        // lookup by id this should be doable in O(n).
        for d in self.by_id.values() {
            let path_file_name = path
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();

            if d.visible_name == path_file_name {
                let d_parent = match d.parent {
                    Parent::Node(p) => Some(p),
                    _ => None,
                };
                match path.parent().zip(d_parent) {
                    None => return Some(d),
                    Some((parent_path, parent_id)) => {
                        match self.get_by_path(parent_path) {
                            None => continue,
                            Some(parent) => {
                                if parent.id == parent_id {
                                    return Some(d);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn children(&self, parent: Parent) -> Vec<&Document> {
        let mut acc: Vec<&Document> = vec![];
        for d in self.by_id.values() {
            if d.parent == parent {
                acc.push(&d);
            }
        }
        acc
    }

    pub fn remove(&mut self, uuid: &Uuid) -> Option<Document> {
        self.by_id.remove(uuid)
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

            fn visit_seq<V>(
                self,
                mut visitor: V,
            ) -> result::Result<Self::Value, V::Error>
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
