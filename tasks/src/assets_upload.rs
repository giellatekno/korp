use crate::cf_models::*;
use crate::manifest::Manifest;

/// The entrypoint to the process of assets upload.
/// There are 3 steps:
/// 1: Send an "assets upload session" request
///    It contains the manifest of files.
///    The manifest contains that path, size, and
///    hash of every file.
///
/// 2: Cloudflare determines the files that have
///    changed, and we only have to send those
///    files.
///    The files are sent as Multipart data,
///    spread over multiple requests.
///    We recieve a list of list of hashes.
///    Each outer list should be a request,
///    and in that request the inner list of
///    hashes is which files to send.
pub async fn deploy_to_cloudflare(
    account_id: &str,
    api_token: &str,
    script_name: &str,
    static_assets_dir: &std::path::Path,
) -> Result<(), UploadAssetsError> {
    match cf_create_worker(account_id, api_token, script_name).await {
        Ok(_) => {}
        Err(e) => {
            if let ReqError::Failed(10040) = e {
                tracing::info!("worker already exists, ok");
            } else {
                return Err(e.into());
            }
        }
    }

    if !static_assets_dir.is_dir() {
        return Err(UploadAssetsError::StaticAssetsDirNotDir(
            static_assets_dir.to_path_buf(),
        ));
    }

    let manifest = Manifest::build_from_directory(static_assets_dir)?;

    //let root = manifest.root.as_path();
    //for (hash, item) in manifest.items {
    //    let Ok(path) = item.path.strip_prefix(root) else {
    //        panic!("at the disco");
    //    };

    //    println!("{hash} {}", path.display());
    //}
    //panic!("temp: ending here now");

    let AssetsUploadSessionResponse {
        jwt: upload_jwt,
        buckets,
    } = send_assets_upload_session_request(account_id, api_token, &manifest, script_name).await?;

    let n = buckets.len();
    let mut i = 0;
    let mut completion_jwt = None;
    if buckets.is_empty() {
        tracing::info!("no changes since last deploy, nothing to upload");
        tracing::info!("got completion jwt");
        completion_jwt = Some(upload_jwt);
    } else {
        for bucket in buckets {
            let bucket = manifest.get_many_by_hash(&bucket)?;
            tracing::info!(i, n, "sending bucket i of n");
            i += 1;

            let UploadResponse { jwt } = upload_assets(account_id, &upload_jwt, bucket).await?;

            if let Some(jwt) = jwt {
                tracing::info!("got completion jwt");
                completion_jwt = Some(jwt);
            }
        }
    }

    let Some(completion_jwt) = completion_jwt else {
        return Err(UploadAssetsError::NoCompletionJwt);
    };

    let versions::Version { id: version_id, .. } =
        create_version(account_id, api_token, script_name, &completion_jwt).await?;

    tracing::info!(version_id, "created new version of worker");

    let resp = create_deployment(account_id, api_token, script_name, &version_id).await?;
    //let Deployment { id: version_id, .. } = resp.try_into_result()?;

    println!("deployed");

    Ok(())
}

/// The errors that can occur during the entire assets upload session.
#[derive(Debug, thiserror::Error)]
pub enum UploadAssetsError {
    #[error("given static assets dir is not a directory ({0})")]
    StaticAssetsDirNotDir(std::path::PathBuf),

    #[error("build manifest error: {0}")]
    BuildManifestError(#[from] crate::manifest::BuildManifestError),

    #[error("error serializing manifest: {0}")]
    ManifestSerialize(#[from] serde_json::Error),

    #[error("reqwest error: {0}")]
    SendStartRequestError(#[from] reqwest::Error),

    #[error("bucket error: {0}")]
    BucketError(#[from] crate::manifest::GetManyByHashError),

    #[error("other request error: {0}")]
    ReqError(#[from] ReqError),

    #[error("cf didn't send completion jwt")]
    NoCompletionJwt,
}

async fn cf_get_worker(
    account_id: &str,
    api_token: &str,
    script_name: &str,
) -> Result<CreateWorkerResponse, ReqError> {
    get(
        &format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/workers/{script_name}"),
        Some(api_token)
    ).await
}

async fn cf_create_worker(
    account_id: &str,
    api_token: &str,
    script_name: &str,
) -> Result<CreateWorkerResponse, ReqError> {
    tracing::info!("creating cloudflare worker");
    post(
        &format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/workers"),
        Some(api_token),
        json_body(CreateWorkerBody {
            name: script_name.to_string(),
            logpush: None,
            observability: None,
            subdomain: Some(CreateWorkerBodySubdomain {
                enabled: Some(true),
                previews_enabled: None,
            }),
            tags: None,
            tail_consumers: None,
        }),
    )
    .await
}

/// Step 1: Send the initial "create assets upload session" request.
async fn send_assets_upload_session_request(
    account_id: &str,
    api_token: &str,
    manifest: &Manifest,
    script_name: &str,
) -> Result<AssetsUploadSessionResponse, ReqError> {
    tracing::info!("sending assets upload request");
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/scripts/{script_name}/assets-upload-session"
    );
    let auth_token = Some(api_token);
    post(&url, auth_token, json_body(manifest.to_request_body())).await
}

/// Step 2: Upload assets for a bucket.
pub async fn upload_assets(
    account_id: &str,
    upload_jwt: &str,
    // hash, content, mime
    files: Vec<(String, String, Option<String>)>,
) -> Result<UploadResponse, ReqError> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/assets/upload?base64=true"
    );

    let mut size = 0;
    // items of the multipart form: name=file_hash, value=base64_file_contents
    let form = files.into_iter().fold(
        reqwest::multipart::Form::new(),
        |form, (hash, content, mime)| {
            size += hash.len() + content.len();
            let mut part = reqwest::multipart::Part::text(content);
            if let Some(mime) = mime {
                part = part.headers(content_type_headermap(&mime));
            }
            form.part(hash, part)
        },
    );

    let size = humansize::format_size(size, humansize::DECIMAL);
    tracing::info!(size, "sending files for bucket");
    post(&url, Some(upload_jwt), Body::Form(form)).await
}

fn content_type_headermap(content_type: &str) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    let value = reqwest::header::HeaderValue::from_str(content_type)
        .expect("mime type returned by the mime_guess library is a valid header value");
    map.insert(reqwest::header::CONTENT_TYPE, value);
    map
}

/// Step 3.1: Create a new *version* of this worker
pub async fn create_version(
    account_id: &str,
    script_name: &str,
    auth_token: &str,
    completion_jwt: &str,
) -> Result<versions::Version, ReqError> {
    tracing::info!("creating new version of the worker");

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/scripts/{script_name}/versions"
    );

    let mut form = reqwest::multipart::Form::new();
    let metadata = versions::CreateVersionMetadata {
        assets: versions::CreateVersionAssets {
            jwt: completion_jwt.to_string(),
        },
    };
    form = form.text("metadata", serde_json::to_string(&metadata).unwrap());

    post(&url, Some(auth_token), Body::Form(form)).await
}

/// Step 3.2: Create a new *deployment* of this version of the worker.
pub async fn create_deployment(
    account_id: &str,
    script_name: &str,
    deploy_token: &str,
    version_id: &str,
) -> Result<Deployment, ReqError> {
    tracing::info!("creating new deployment of the new version");
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{account_id}/workers/scripts/{script_name}/deployments"
    );

    let body = CreateDeploymentBody {
        versions: vec![DeploymentVersion {
            percentage: 100.0,
            version_id: version_id.to_string(),
        }],
    };

    post(&url, Some(deploy_token), json_body(body)).await
}

// convenience functions to not have to repeat so much code

fn json_body<T: serde::Serialize>(obj: T) -> Body {
    let v = serde_json::to_value(&obj).expect("T is serialize, so it can serialize");
    Body::Json(v)
}

pub enum Body {
    Form(reqwest::multipart::Form),
    Json(serde_json::Value),
}

pub async fn get<R>(url: &str, auth_token: Option<&str>) -> Result<R, ReqError>
where
    R: serde::de::DeserializeOwned,
{
    let mut rb = reqwest::Client::new().post(url);

    if let Some(auth_token) = auth_token {
        rb = rb.header("Authorization", format!("Bearer {}", auth_token));
    }

    let resp = rb.send().await?;
    let text = resp.text().await?;

    let api_response = match serde_json::from_str::<ApiResponse<R>>(&text) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!(text, "could not deserialize body into type");
            return Err(ReqError::Deserialize(e));
        }
    };

    api_response.try_into_result()
}

pub async fn post<R>(url: &str, auth_token: Option<&str>, body: Body) -> Result<R, ReqError>
where
    R: serde::de::DeserializeOwned,
{
    let mut rb = reqwest::Client::new().post(url);

    if let Some(auth_token) = auth_token {
        rb = rb.header("Authorization", format!("Bearer {}", auth_token));
    }

    match body {
        Body::Form(form) => {
            rb = rb.multipart(form);
        }
        Body::Json(json) => {
            rb = rb.header("Content-Type", "application/json");
            rb = rb.json(&json);
        }
    }

    let resp = rb.send().await?;
    let text = resp.text().await?;
    let api_response = match serde_json::from_str::<ApiResponse<R>>(&text) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!(text, "could not deserialize body into type");
            return Err(ReqError::Deserialize(e));
        }
    };

    api_response.try_into_result()
}
