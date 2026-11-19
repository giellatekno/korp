//! Container images and containers, by running the `docker` command.

use crate::childprocess::{SubprocessError, child_exit_ok};

/// The container tool. Always docker: Cloudflare's tooling (wrangler) uses docker,
/// and podman builds/pushes images differently enough to cause trouble (e.g.
/// recompressed layers, so the registry digest differs from the local one).
pub const DOCKER: &str = "docker";

/// <registry>/<name>:<tag>
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageName {
    pub registry: String,
    pub name: String,
    pub tag: String,
}

#[derive(Default)]
pub struct ImageNameBuilder {
    registry: Option<String>,
    name: Option<String>,
    tag: Option<String>,
}

impl ImageName {
    pub fn builder() -> ImageNameBuilder {
        ImageNameBuilder::default()
    }

    pub fn to_name_and_tag_string(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

impl ImageNameBuilder {
    pub fn registry(mut self, val: String) -> Self {
        self.registry = Some(val);
        self
    }

    pub fn name(mut self, val: String) -> Self {
        if val.contains("/") {
            panic!("name can't contain /");
        }
        if val.contains(":") {
            panic!("name can't contain :");
        }
        self.name = Some(val);
        self
    }

    pub fn tag(mut self, val: String) -> Self {
        if val.contains(":") {
            panic!("tag can't contain :");
        }
        self.tag = Some(val);
        self
    }

    pub fn build(mut self) -> Result<ImageName, String> {
        let registry = self
            .registry
            .take()
            .unwrap_or_else(|| "localhost".to_string());
        let name = self
            .name
            .take()
            .ok_or_else(|| "name must be set".to_string())?;
        let tag = self.tag.take().unwrap_or_else(|| "latest".to_string());

        Ok(ImageName {
            registry,
            name,
            tag,
        })
    }
}

impl std::fmt::Display for ImageName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}/{}:{}", self.registry, self.name, self.tag)
    }
}

/// Execute `docker cp <src> <dst>`.
pub async fn copy(src: &str, dst: &str) -> Result<(), SubprocessError> {
    let status = std::process::Command::new(DOCKER)
        .args(["cp", src, dst])
        .status()?;
    child_exit_ok(&status)?;
    Ok(())
}

/// Execute `docker stop <container_name>`.
pub async fn stop(container_name: &str) -> Result<(), SubprocessError> {
    let status = std::process::Command::new(DOCKER)
        .arg("stop")
        .arg(container_name)
        .status()?;
    child_exit_ok(&status)?;
    Ok(())
}

/// Execute `docker run --rm -i --detach [--name <name>] <image>`, and return the
/// container id.
pub async fn run_detached(image: &str, name: Option<&str>) -> Result<String, SubprocessError> {
    let mut cmd = std::process::Command::new(DOCKER);
    cmd.arg("run");
    cmd.arg("--rm");
    cmd.arg("-i");
    cmd.arg("--detach");
    if let Some(name) = name {
        cmd.arg("--name");
        cmd.arg(name);
    }
    cmd.arg(image);
    let output = cmd.output()?;
    child_exit_ok(&output.status)?;
    let mut stdout = output.stdout;
    stdout.retain(|b| b.is_ascii_hexdigit());
    Ok(String::from_utf8(stdout)?)
}
