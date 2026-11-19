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
                if error_code.is_none() {
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
