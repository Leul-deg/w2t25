use gloo_net::http::Request;
use serde::{de::DeserializeOwned, Serialize};

pub fn build_api_base_for_host(host: &str) -> String {
    format!("http://{}:8080/api/v1", host)
}

fn api_base() -> String {
    if let Some(explicit) = option_env!("API_BASE_URL") {
        return explicit.to_string();
    }

    let host = web_sys::window()
        .and_then(|w| w.location().hostname().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".to_string());

    build_api_base_for_host(&host)
}

#[derive(Debug)]
pub enum ApiError {
    Network(String),
    Deserialize(String),
    Http { status: u16, message: String },
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Network(e) => write!(f, "Network error: {}", e),
            ApiError::Deserialize(e) => write!(f, "Parse error: {}", e),
            ApiError::Http { status, message } => write!(f, "HTTP {}: {}", status, message),
        }
    }
}

pub async fn get<T: DeserializeOwned>(path: &str, token: Option<&str>) -> Result<T, ApiError> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::get(&url);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    if resp.status() >= 400 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(ApiError::Http {
            status: resp.status(),
            message: msg,
        });
    }
    resp.json::<T>()
        .await
        .map_err(|e| ApiError::Deserialize(e.to_string()))
}

pub async fn post<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
    token: Option<&str>,
) -> Result<T, ApiError> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::post(&url).header("Content-Type", "application/json");
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    let body_str =
        serde_json::to_string(body).map_err(|e| ApiError::Network(e.to_string()))?;
    let resp = req
        .body(body_str)
        .map_err(|e| ApiError::Network(e.to_string()))?
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    if resp.status() >= 400 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(ApiError::Http {
            status: resp.status(),
            message: msg,
        });
    }
    resp.json::<T>()
        .await
        .map_err(|e| ApiError::Deserialize(e.to_string()))
}

pub async fn patch<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
    token: Option<&str>,
) -> Result<T, ApiError> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::patch(&url).header("Content-Type", "application/json");
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    let body_str =
        serde_json::to_string(body).map_err(|e| ApiError::Network(e.to_string()))?;
    let resp = req
        .body(body_str)
        .map_err(|e| ApiError::Network(e.to_string()))?
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    if resp.status() >= 400 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(ApiError::Http {
            status: resp.status(),
            message: msg,
        });
    }
    resp.json::<T>()
        .await
        .map_err(|e| ApiError::Deserialize(e.to_string()))
}

pub async fn post_no_body(path: &str, token: Option<&str>) -> Result<(), ApiError> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::post(&url);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    if resp.status() >= 400 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(ApiError::Http {
            status: resp.status(),
            message: msg,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_api_base_for_host;

    #[test]
    fn builds_localhost_api_base() {
        assert_eq!(
            build_api_base_for_host("localhost"),
            "http://localhost:8080/api/v1"
        );
    }

    #[test]
    fn builds_lan_ip_api_base() {
        assert_eq!(
            build_api_base_for_host("192.168.1.25"),
            "http://192.168.1.25:8080/api/v1"
        );
    }
}
