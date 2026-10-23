//! A tiny in-process HTTP server standing in for ComfyUI in tests.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct TestRoute {
  method: &'static str,
  /// Matched as a prefix of the request path (including the query string).
  path_prefix: String,
  status: u16,
  content_type: &'static str,
  body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CapturedRequest {
  pub method: String,
  pub path: String,
  pub content_type: String,
  pub body: Vec<u8>,
}

impl TestRoute {
  pub fn json(method: &'static str, path_prefix: &str, status: u16, body: &str) -> Self {
    Self::bytes(method, path_prefix, status, "application/json", body.as_bytes().to_vec())
  }

  pub fn bytes(method: &'static str, path_prefix: &str, status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
    Self { method, path_prefix: path_prefix.to_string(), status, content_type, body }
  }
}

/// Serve `routes` until the test ends; unmatched requests get a 404.
pub async fn spawn_test_server(routes: Vec<TestRoute>) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind localhost");
  let base_url = format!("http://{}", listener.local_addr().unwrap());
  let captured = Arc::new(Mutex::new(Vec::new()));
  let routes = Arc::new(routes);

  let captured_for_task = captured.clone();
  tokio::spawn(async move {
    loop {
      let Ok((mut socket, _)) = listener.accept().await else { return };
      let request = read_request(&mut socket).await;
      let route = routes.iter().find(|r| r.method == request.method && request.path.starts_with(&r.path_prefix));
      let (status, content_type, body) = match route {
        Some(route) => (route.status, route.content_type, route.body.clone()),
        None => (404, "text/plain", b"not found".to_vec()),
      };
      captured_for_task.lock().unwrap().push(request);
      let head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status, content_type, body.len(),
      );
      let _ = socket.write_all(head.as_bytes()).await;
      let _ = socket.write_all(&body).await;
      let _ = socket.shutdown().await;
    }
  });

  (base_url, captured)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> CapturedRequest {
  let mut buffer = Vec::new();
  let header_end = loop {
    let mut chunk = [0u8; 8192];
    let n = socket.read(&mut chunk).await.expect("read");
    assert!(n > 0, "connection closed before headers finished");
    buffer.extend_from_slice(&chunk[..n]);
    if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
      break pos + 4;
    }
  };

  let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
  let mut lines = head.split("\r\n");
  let mut request_line = lines.next().unwrap_or_default().split(' ');
  let method = request_line.next().unwrap_or_default().to_string();
  let path = request_line.next().unwrap_or_default().to_string();
  let headers: Vec<(String, String)> = lines
    .filter_map(|line| line.split_once(':'))
    .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
    .collect();
  let header = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default();
  let content_length: usize = header("content-length").parse().unwrap_or(0);

  let mut body = buffer[header_end..].to_vec();
  while body.len() < content_length {
    let mut chunk = [0u8; 8192];
    let n = socket.read(&mut chunk).await.expect("read body");
    if n == 0 {
      break;
    }
    body.extend_from_slice(&chunk[..n]);
  }

  CapturedRequest { method, path, content_type: header("content-type"), body }
}
