//! Media on the local filesystem: `<dir>/<id>` holds the bytes and `<dir>/<id>.json`
//! the metadata. Good for one box (back the directory up with the database); swap in
//! an object store when the app runs on more than one machine.

use async_trait::async_trait;
use social_core::{Error, MediaId, MediaMeta, Result};
use social_storage::MediaStore;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub struct FsMediaStore {
    dir: PathBuf,
}

impl FsMediaStore {
    /// Creates `dir` if needed.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(|e| Error::Internal(format!("media dir {}: {e}", dir.display())))?;
        Ok(Self { dir })
    }

    fn data_path(&self, id: MediaId) -> PathBuf {
        self.dir.join(id.to_string())
    }

    fn meta_path(&self, id: MediaId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

fn io(e: std::io::Error) -> Error {
    Error::Internal(format!("media store: {e}"))
}

/// Writes via a temporary file and a rename, so readers never see a partial file.
async fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, data).await.map_err(io)?;
    tokio::fs::rename(&tmp, path).await.map_err(io)
}

async fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match tokio::fs::read(path).await {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(e)),
    }
}

#[async_trait]
impl MediaStore for FsMediaStore {
    async fn put(&self, meta: &MediaMeta, data: &[u8]) -> Result<()> {
        write_atomic(&self.data_path(meta.id), data).await?;
        // Metadata last: an image exists once its metadata does.
        let json = serde_json::to_vec(meta).map_err(|e| Error::Internal(e.to_string()))?;
        write_atomic(&self.meta_path(meta.id), &json).await
    }

    async fn meta(&self, id: MediaId) -> Result<Option<MediaMeta>> {
        match read_optional(&self.meta_path(id)).await? {
            Some(b) => serde_json::from_slice(&b).map(Some).map_err(|e| Error::Internal(e.to_string())),
            None => Ok(None),
        }
    }

    async fn get(&self, id: MediaId) -> Result<Option<(MediaMeta, Vec<u8>)>> {
        let Some(meta) = self.meta(id).await? else { return Ok(None) };
        Ok(read_optional(&self.data_path(id)).await?.map(|data| (meta, data)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use social_core::UserId;

    #[tokio::test]
    async fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsMediaStore::open(dir.path().join("media")).unwrap();
        let meta = MediaMeta {
            id: MediaId(7),
            owner: UserId(1),
            content_type: "image/png".into(),
            width: 2,
            height: 3,
            bytes: 4,
        };
        assert!(store.get(meta.id).await.unwrap().is_none());
        store.put(&meta, b"data").await.unwrap();
        assert_eq!(store.meta(meta.id).await.unwrap(), Some(meta.clone()));
        assert_eq!(store.get(meta.id).await.unwrap(), Some((meta, b"data".to_vec())));
    }
}
