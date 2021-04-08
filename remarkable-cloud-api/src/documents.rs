use std::collections::HashMap;
use std::fmt;
use std::path;
use std::result;

use serde::de::Deserialize;
pub use uuid::Uuid;

#[derive(Debug, serde::Deserialize)]
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
fn deserialize_optional_uuid<'de, D>(
    deserializer: D,
) -> result::Result<Option<Uuid>, D::Error>
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

    pub fn get_children(&self, uuid: &Option<Uuid>) -> Vec<&Document> {
        let mut acc: Vec<&Document> = vec![];
        for d in self.by_id.values() {
            if d.parent == *uuid {
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
