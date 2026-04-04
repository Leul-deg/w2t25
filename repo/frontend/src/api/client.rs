use gloo_net::http::Request;
use serde::{de::DeserializeOwned, Serialize};

pub const API_BASE: &str = "http://localhost:8080/api/v1";

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
    let url = format!("{}{}", API_BASE, path);
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
    let url = format!("{}{}", API_BASE, path);
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
    let url = format!("{}{}", API_BASE, path);
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
    let url = format!("{}{}", API_BASE, path);
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
