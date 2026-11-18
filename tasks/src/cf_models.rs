#[derive(Debug, thiserror::Error)]
pub enum ReqError {
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("query failed, error code: {0}")]
    Failed(u64),

    #[error("deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
}

/// The generic ApiResponse, all responses are of this type.
#[derive(Debug, serde::Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,

    pub result: Option<T>,

    #[serde(default)]
    pub messages: Option<Vec<Message>>,

    #[serde(default)]
    pub errors: Option<Vec<Message>>,
}

impl<T> ApiResponse<T> {
    /// Convenience function
    pub fn try_into_result(self) -> Result<T, ReqError> {
        // log any messages and errors we got
        if let Some(ref messages) = self.messages {
            for message in messages {
                tracing::info!(?message, "message from api response");
            }
        }

        let mut error_code = None;
        if let Some(ref errors) = self.errors {
            for error in errors {
                if let None = error_code {
                    error_code = Some(error.code);
                }
                tracing::error!(?error, "error message");
            }
        }

        // return the inner result, but
        match (self.success, self.result) {
            (true, Some(result)) => Ok(result),
            (false, None) => {
                let error_code = error_code.expect("error code always given when success=false");
                Err(ReqError::Failed(error_code))
            }
            (true, None) => panic!("success but no result, what to return?"),
            (false, Some(_result)) => panic!("success=false but results object given?"),
        }
    }
}

/// The generic "message", the form of messages and errors
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
pub struct Message {
    // probably is really at most u32, maybe even just u16
    pub code: u64,
    pub message: String,

    #[serde(default)]
    pub documentation_url: Option<String>,

    #[serde(default)]
    pub source: SourceObj,
}

#[derive(Debug, Default, serde::Deserialize)]
#[allow(dead_code)]
pub struct SourceObj {
    pub pointer: String,
}

/// The response from the initial "create assets upload session" request.
#[derive(Debug, serde::Deserialize)]
pub struct AssetsUploadSessionResponse {
    /// The jwt to use for uploads.
    pub jwt: String,

    /// list of which files should be sent in individual uploads.
    /// e.g. (hashes are longer, and lists are probably same length)```
    /// [
    ///     ["08f1dfda", "c21666d1"],
    ///     ["620d5314", "4f1c1a44", "46ceef93f", "23757a04ec1"],
    ///     ["54995e302"]
    /// ]
    /// ```
    pub buckets: Vec<Vec<String>>,
}

/// Response from individual asset upload request.
#[derive(Debug, serde::Deserialize)]
pub struct UploadResponse {
    pub jwt: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWorkerBody {
    pub name: String,

    /// Whether logpush is enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logpush: Option<bool>,

    /// Observability settings for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability: Option<CreateWorkerBodyObservability>,

    /// Subdomain settings for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<CreateWorkerBodySubdomain>,

    /// Tags associated with the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    /// Other Workers that should consume logs from the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_consumers: Option<Vec<CreateWorkerBodyTailConsumer>>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWorkerBodyObservability {
    /// Whether observability is enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The sampling rate for observability. From 0 to 1 (1 = 100%, 0.1 = 10%).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sampling_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<CreateWorkerBodyObservabilityLogs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traces: Option<CreateWorkerBodyObservabilityTraces>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWorkerBodyObservabilityLogs {
    /// A list of destinations where logs will be exported to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destinations: Option<Vec<String>>,
    /// Whether logs are enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The sampling rate for logs. From 0 to 1 (1 = 100%, 0.1 = 10%).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sampling_rate: Option<f64>,
    /// Whether
    /// [invocation logs](https://developers.cloudflare.com/workers/observability/logs/workers-logs/#invocation-logs)
    /// are enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation_logs: Option<bool>,
    /// Whether log persistence is enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWorkerBodyObservabilityTraces {
    /// A list of destinations where traces will be exported to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destinations: Option<Vec<String>>,

    /// Whether traces are enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// The sampling rate for traces. From 0 to 1 (1 = 100%, 0.1 = 10%).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sampling_rate: Option<f64>,

    /// Whether trace persistence is enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist: Option<bool>,

    /// Controls how inbound trace context (traceparent/tracestate) headers on incoming
    /// requests are handled. "authenticated" (default) honors inbound trace context
    /// only when accompanied by a valid trace auth token. "accept" unconditionally
    /// accepts inbound trace context. Requires the trace propagation feature to be
    /// enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propagation_policy: Option<String>, // 'authenticated' | 'accept'
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWorkerBodySubdomain {
    /// Whether the \*.workers.dev subdomain is enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Whether
    /// [preview URLs](https://developers.cloudflare.com/workers/configuration/previews/)
    /// are enabled for the Worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previews_enabled: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWorkerBodyTailConsumer {
    /// Name of the consumer Worker.
    pub name: String,
}

/// Response from "create worker" endpoint (POST /accounts/{account_id}/workers/worker)
/// {
///    "id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///    "created_on": "2019-12-27T18:11:19.117Z",
///    "logpush": true,
///    "name": "my-worker",
///    "observability": {
///      "enabled": true,
///      "head_sampling_rate": 1,
///      "logs": {
///        "destinations": [
///          "string"
///        ],
///        "enabled": true,
///        "head_sampling_rate": 1,
///        "invocation_logs": true,
///        "persist": true
///      },
///      "traces": {
///        "destinations": [
///          "string"
///        ],
///        "enabled": true,
///        "head_sampling_rate": 1,
///        "persist": true,
///        "propagation_policy": "authenticated"
///      }
///    },
///    "references": {
///      "dispatch_namespace_outbounds": [
///        {
///          "namespace_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "namespace_name": "my-dispatch-namespace",
///          "worker_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "worker_name": "my-worker"
///        }
///      ],
///      "domains": [
///        {
///          "id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "certificate_id": "certificate_id",
///          "hostname": "my-worker.example.com",
///          "zone_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "zone_name": "example.com"
///        }
///      ],
///      "durable_objects": [
///        {
///          "namespace_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "namespace_name": "my-durable-object-namespace",
///          "worker_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "worker_name": "my-worker"
///        }
///      ],
///      "queues": [
///        {
///          "queue_consumer_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "queue_id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "queue_name": "my-queue"
///        }
///      ],
///      "workers": [
///        {
///          "id": "e8f70fdbc8b1fb0b8ddb1af166186758",
///          "name": "my-worker"
///        }
///      ]
///    },
///    "subdomain": {
///      "enabled": true,
///      "preview_url_suffix": "-my-worker.my-subdomain.workers.dev",
///      "previews_enabled": true,
///      "url": "https://my-worker.my-subdomain.workers.dev"
///    },
///    "tags": [
///      "my-team",
///      "my-public-api"
///    ],
///    "tail_consumers": [
///      {
///        "name": "my-tail-consumer"
///      }
///    ],
///    "updated_on": "2019-12-27T18:11:19.117Z",
///    "deployed_on": "2019-12-27T18:11:19.117Z"
///  },
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
pub struct CreateWorkerResponse {
    pub id: String,
    pub created_on: String,
    pub logpush: bool,
    pub name: String,
    pub observability: Option<CreateWorkerObservability>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateWorkerObservability {}

pub mod versions {
    /// Metadata-field of the multipart body when creating a new version
    #[derive(Debug, serde::Serialize)]
    pub struct CreateVersionMetadata {
        pub assets: CreateVersionAssets,
    }

    /// "assets" field of CreateVersion
    #[derive(Debug, serde::Serialize)]
    pub struct CreateVersionAssets {
        /// The completion jwt
        pub jwt: String,
    }

    /// Response when creating a new version of a worker.
    #[allow(dead_code)]
    #[derive(Debug, serde::Deserialize)]
    pub struct Version {
        /// Version identifier.
        pub id: String,

        /// The integer version number, starting from one.
        #[serde(default)]
        pub number: Option<u64>,

        #[serde(default)]
        pub metadata: Option<VersionMetadata>,
    }

    // // All routable URLs that always point to this version. Does not include
    // // alias URLs, since aliases can be updated to point to a different version.
    // pub urls: Vec<String>,

    // #[serde(default)]
    // pub compatability_date: Option<String>,

    // #[serde(default)]
    // pub compatability_flags: Option<Vec<String>>,
    // // and a whole heap of additional stuff that I haven't bothered with here...
    #[allow(dead_code)]
    #[derive(Debug, serde::Deserialize)]
    pub struct VersionMetadata {
        /// "created_on": "2026-08-10T08:41:09.475816Z",
        pub created_on: String,
        /// "modified_on": "2026-08-10T08:41:09.475816Z",
        pub modified_on: String,
        /// "source": "api",
        pub source: String,
        /// "author_id": "c836c040c0adbac64c26199c39128c5d",
        pub author_id: String,
        /// "author_email": "",
        pub author_email: String,
        /// "has_preview": true
        pub has_preview: bool,
    }
}

/// Returned when creating a new deployment. Also when listing deployments.
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
pub struct Deployment {
    pub id: String,
    //pub created_on: String,
    //pub source: String,
    //pub strategy: String,
    //pub versions: DeploymentVersion,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct DeploymentVersion {
    pub percentage: f32,
    pub version_id: String,
}

/// Body to be sent as json when creating a new deployment
#[derive(Debug, serde::Serialize)]
pub struct CreateDeploymentBody {
    pub versions: Vec<DeploymentVersion>,
}
