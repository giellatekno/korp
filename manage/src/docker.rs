//! Container images and containers, by running the `docker` command.

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
