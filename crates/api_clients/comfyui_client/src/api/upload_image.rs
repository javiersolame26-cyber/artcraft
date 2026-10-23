use serde_derive::Deserialize;

use crate::error::ComfyUiError;

/// An input image stored on the ComfyUI server by `POST /upload/image`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UploadedImage {
  pub name: String,

  #[serde(default)]
  pub subfolder: String,

  #[serde(rename = "type", default)]
  pub folder_type: String,
}

impl UploadedImage {
  /// The value a `LoadImage` node expects for its `image` input.
  pub fn load_image_value(&self) -> String {
    if self.subfolder.is_empty() {
      self.name.clone()
    } else {
      format!("{}/{}", self.subfolder, self.name)
    }
  }
}

pub fn parse_uploaded_image(body: &str) -> Result<UploadedImage, ComfyUiError> {
  serde_json::from_str(body).map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn load_image_value_includes_subfolder() {
    let image = parse_uploaded_image(r#"{"name": "scene.png", "subfolder": "artcraft", "type": "input"}"#).unwrap();
    assert_eq!(image.load_image_value(), "artcraft/scene.png");
  }

  #[test]
  fn load_image_value_without_subfolder() {
    let image = parse_uploaded_image(r#"{"name": "scene.png", "subfolder": "", "type": "input"}"#).unwrap();
    assert_eq!(image.load_image_value(), "scene.png");
  }
}
