use std::path::Path;

use jsonc_parser::cst::{CstRootNode, CstStringLit};

use crate::docker::ImageName;

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parsing jsonc failed: {0}")]
    Parse(#[from] jsonc_parser::errors::ParseError),

    #[error("wrangler.jsonc was empty")]
    Empty,

    #[error("wrangler.jsonc schema error: {0}")]
    Schema(String),
}

pub struct Document {
    inner: CstRootNode,
}

impl Document {
    pub fn from_path(path: &Path) -> Result<Self, DocumentError> {
        let s = std::fs::read_to_string(path)?;
        let inner = CstRootNode::parse(&s, &Default::default())?;
        Ok(Self { inner })
    }

    pub fn get_image(&self) -> Result<ImageName, DocumentError> {
        let image_string = self.image_string_ref()?.decoded_value().map_err(|e| {
            DocumentError::Schema(format!("can't decode string in wrangler.jsonc: {e}"))
        })?;

        let Ok(image_name) = crate::docker::ImageName::try_from(image_string.as_str()); /* else {
        // TODO: irrefutable Err variant since for now parseimagenameerror is empty
        return Err(ChangeTagError::Schema("can't parse image name in .containers[0].image".to_string()));
        };*/
        Ok(image_name)
    }

    pub fn set_image(&mut self, new_image: ImageName) -> Result<(), DocumentError> {
        self.image_string_ref()?
            .replace_with(jsonc_parser::json!(new_image.to_string()));
        Ok(())
    }

    fn image_string_ref(&self) -> Result<CstStringLit, DocumentError> {
        let Some(root) = self.inner.object_value() else {
            return Err(DocumentError::Schema(
                "top level element is not an object".to_string(),
            ));
        };

        let Some(containers) = root.array_value("containers") else {
            return Err(DocumentError::Schema("no .containers".to_string()));
        };

        let mut children = containers.children_exclude_trivia_and_tokens();
        if children.is_empty() {
            return Err(DocumentError::Schema("no .containers[0]".to_string()));
        }

        let first = children.swap_remove(0);

        let Some(first) = first.as_object() else {
            return Err(DocumentError::Schema(
                ".containers[0] is not an object".to_string(),
            ));
        };

        let image_obj = first
            .get("image")
            .ok_or_else(|| DocumentError::Schema("no .containers[0].image".to_string()))?;

        let image_val = image_obj.value().expect("all object properties has values");
        let Some(image_s) = image_val.as_string_lit() else {
            return Err(DocumentError::Schema(
                ".containers[0].image is not a string, as expected".to_string(),
            ));
        };
        Ok(image_s)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, format!("{}", self.inner))
    }
}
