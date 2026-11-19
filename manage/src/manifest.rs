use std::collections::hash_map::{Entry, HashMap};
use std::path::{Path, PathBuf};

use crate::base64::base64_encode;

/// The manifest of files we are sending
#[allow(dead_code)]
pub struct Manifest {
    /// The absolute path to the html directory
    pub root: PathBuf,

    /// Hashmap of hash -> item, for easy lookup
    pub items: HashMap<String, ManifestItem>,
}

#[allow(dead_code)]
pub struct ManifestItem {
    /// Full path to the file.
    pub path: PathBuf,

    /// Detected mime type of the file, which we send to CF, to have
    /// them serve this file with "content-type: mimetype" header
    /// (which we need for images, for example)
    pub mime: Option<String>,

    /// Relative path within the `html` directory, as a [String], with
    /// a prepended "/", i.e. the form the cloudflare wants it.
    pub rel_path: String,

    /// Size of the file, in bytes.
    pub size: u64,

    /// Base64-encoded file data.
    pub contents: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildManifestError {
    #[error("error walking directory: {0}")]
    WalkdirError(#[from] walkdir::Error),

    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("file path in `html` dir is not utf8: {0}")]
    NonUtf8Path(PathBuf),
}

impl Manifest {
    /// Build the manifest from the given directory, which should be the
    /// `html` directory of built files extracted from the `korp-frontend-xxx`
    /// image.
    pub fn build_from_directory<P: AsRef<Path>>(dir: P) -> Result<Self, BuildManifestError> {
        assert!(dir.as_ref().is_dir());
        let mut items = HashMap::new();

        for maybe_entry in walkdir::WalkDir::new(&dir) {
            let entry = maybe_entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let contents = base64_encode(&std::fs::read(path)?);
            let size = entry.metadata()?.len();
            let Ok(rel_path) = path.strip_prefix(&dir) else {
                panic!("all walkdir entries have a prefix of the root");
            };
            let Some(path_str) = rel_path.to_str() else {
                return Err(BuildManifestError::NonUtf8Path(rel_path.to_path_buf()));
            };
            let hash = hash_file(path)?;
            let rel_path = format!("/{path_str}");

            let mime = mime_guess::from_path(path)
                .first()
                .map(|mime| mime.essence_str().to_string());

            let item = ManifestItem {
                path: path.to_path_buf(),
                mime,
                rel_path,
                size,
                contents,
            };

            match items.entry(hash) {
                Entry::Vacant(entry) => entry.insert(item),
                Entry::Occupied(_) => panic!("hash collisions in static dir"),
            };
        }

        Ok(Self {
            root: dir.as_ref().to_path_buf(),
            items,
        })
    }

    /// Create a copy of the manifest, in the form the "asset upload session"
    /// request wants as the body.
    pub fn to_request_body(&self) -> BodyManifest {
        let manifest = HashMap::from_iter(self.items.iter().map(|(hash, item)| {
            let path = item.rel_path.to_string();
            let body_item = BodyManifestItem {
                hash: hash.to_string(),
                size: item.size,
            };
            (path, body_item)
        }));
        BodyManifest { manifest }
    }

    /// Helper function used in deploy_to_cloudflare(), to find the file content
    /// and mime type of each file identified by the hash given in the `hashes`
    /// argument.
    pub fn get_many_by_hash(
        &self,
        hashes: &[String],
    ) -> Result<Vec<(String, String, Option<String>)>, GetManyByHashError> {
        let mut out = Vec::with_capacity(hashes.len());
        for hash in hashes {
            let Some(item) = self.items.get(hash) else {
                return Err(GetManyByHashError::InvalidHash(hash.to_string()));
            };
            let out_item = (
                hash.to_string(),
                item.contents.to_string(),
                item.mime.clone(),
            );
            out.push(out_item);
        }
        Ok(out)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GetManyByHashError {
    #[error("cloudflare gave us an invalid hash that wasn't found: {0}")]
    InvalidHash(String),
}

/// The version of the manifest used as the body to the asset upload session
/// request.
/// curl -X POST https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/scripts/{script_name}/assets-upload-session \
/// --header 'content-type: application/json' \
/// --header 'Authorization: Bearer <API_TOKEN>' \
/// --data '{
///   "manifest": {
///       "/filea.html": {
///           "hash": "08f1dfda4574284ab3c21666d1",
///           "size": 12
///       },
///       "/fileb.html": {
///           "hash": "4f1c1af44620d531446ceef93f",
///           "size": 23
///       },
///       "/filec.html": {
///           "hash": "54995e302614e0523757a04ec1",
///           "size": 23
///       }
///   }
/// }'
#[derive(serde::Serialize)]
pub struct BodyManifest {
    // relative path, starting with "/" -> to object of hash and size
    manifest: HashMap<String, BodyManifestItem>,
}

#[derive(serde::Serialize)]
pub struct BodyManifestItem {
    pub hash: String,
    pub size: u64,
}

/// Create the hash string, the way cloudflare did it. Why they did it
/// exactly this way, I do not know. Also, the hash may potentially be
/// just something they store from this endpoint, so that we can do whatever
/// we want. I may experiment later.
///
/// https://developers.cloudflare.com/workers/static-assets/direct-upload/
///
/// ```javascript
/// // Generate a hash for the file
/// const hash = crypto
///     .createHash("sha256")
///     .update(fileContent.toString("base64") + extension)
///     .digest("hex")
///     .slice(0, 32);
/// ```
fn hash_file<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};

    let contents = std::fs::read(path.as_ref())?;
    let mut contents = base64_encode(&contents);

    // After the base64-ed content of the file, concatenate the full path.
    // In cf's js example code, they concatanate the extension of the path
    // to the file - but in our case here, we have multiple paths with the
    // same extention that are equal, leading to the same hash for different
    // files (files with the same extension, containing the same data).
    // Why korp-frontend builds the same files under different names, I do
    // not now.
    let path_str = path.as_ref().to_str().expect(
        "paths are valid utf-8, already checked in parent function (build_from_directory())",
    );
    //let extension = nodejs_compat_extension(path.as_ref())
    //    .ok_or_else(|| std::io::Error::other("file extension not utf8"))?;
    contents.push_str(path_str);

    let hashed_base64 = Sha256::digest(contents)
        .into_iter()
        .take(16)
        .collect::<Vec<u8>>();

    Ok(hex::encode(hashed_base64))
}

// /// Return the extension of path as a [String], following the nodejs
// /// convention of leaving on the last dot. Returns [None] if the extension
// /// is not valid unicode, and the empty string for paths with no extension.
// pub fn nodejs_compat_extension<P: AsRef<Path>>(path: P) -> Option<String> {
//     // node retains the "." in the extension, but otherwise should be same as
//     // std::path::extension
//     match path.as_ref().extension() {
//         Some(ext) => {
//             let s = ext.to_str()?;
//             Some(format!(".{s}"))
//         }
//         None => Some(String::new()),
//     }
// }
