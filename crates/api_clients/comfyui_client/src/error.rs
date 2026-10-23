use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComfyUiError {
  /// The configured server address can't be used as a base URL.
  #[error("invalid ComfyUI address `{0}`")]
  InvalidBaseUrl(String),

  /// The server could not be reached (not running, wrong port, firewall).
  #[error("could not reach ComfyUI: {0}")]
  Unreachable(reqwest::Error),

  /// Any other transport error while talking to the server.
  #[error("HTTP error talking to ComfyUI: {0}")]
  Http(#[from] reqwest::Error),

  /// The server answered with an unexpected HTTP status.
  #[error("ComfyUI returned HTTP {status}: {body}")]
  BadStatus { status: u16, body: String },

  /// ComfyUI refused to queue the workflow (missing model file, unknown node, bad input).
  #[error("ComfyUI rejected the workflow: {message}")]
  PromptRejected { message: String, node_errors: String },

  /// The server answered, but not with the JSON shape we expected.
  #[error("could not parse ComfyUI response ({error}): {body}")]
  Parse { error: serde_json::Error, body: String },
}

impl ComfyUiError {
  pub(crate) fn from_send_error(err: reqwest::Error) -> Self {
    if err.is_connect() || err.is_timeout() {
      Self::Unreachable(err)
    } else {
      Self::Http(err)
    }
  }

  /// True when the server could not be reached at all, as opposed to an
  /// error reported by a running server.
  pub fn is_unreachable(&self) -> bool {
    matches!(self, Self::Unreachable(_))
  }
}
