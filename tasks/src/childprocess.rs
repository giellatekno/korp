// Functionality to be able to check the exit status of a child process,
// and return an error if the child did not exit cleanly (non-zero, or non-existant exit
// code)

/// There was some error when running a subprocess. Process could have failed to started,
/// or there was a failure when waiting for the child to exit, indicated by `Io`, wrapping
/// an underying `std::io::Error`. Output is assumed to be utf-8, so that's another variant.
/// Or, if the exit status was not success.
#[derive(Debug, thiserror::Error)]
pub enum SubprocessError {
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not decode output as utf8: {0}")]
    Utf8Decode(#[from] std::string::FromUtf8Error),

    #[error("child process exit failure: {0}")]
    ChildExit(#[from] ChildExitError),
}

#[derive(Debug, thiserror::Error)]
#[error("Child did not exit succesfully: {0}")]
pub enum ChildExitError {
    #[error("Non-zero exit code: {0}")]
    NonZero(i32),

    #[error("Terminated by signal or otherwise")]
    Terminated,
}

/// Helper function to return a `ChildExitError` error if the given `ExitStatus`
/// was not success.
pub fn child_exit_ok(exit_status: &std::process::ExitStatus) -> Result<(), ChildExitError> {
    match exit_status.code() {
        Some(code) if code == 0 => Ok(()),
        Some(code) => Err(ChildExitError::NonZero(code)),
        None => Err(ChildExitError::Terminated),
    }
}
