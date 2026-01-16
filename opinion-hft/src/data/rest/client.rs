use reqwest::{Client, Response};
use serde::{de::DeserializeOwned, Serialize};

use crate::config::ApiConfig;
use crate::error::{Error, Result};

pub struct RestClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl RestClient {
    pub fn new(config: &ApiConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .pool_max_idle_per_host(10)
            .http2_prior_knowledge()
            .build()
            .map_err(|e| Error::ConfigError(format!("failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: config.rest_base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
        })
    }

    fn url_with_api_key(&self, endpoint: &str) -> String {
        let separator = if endpoint.contains('?') { '&' } else { '?' };
        format!(
            "{}/{}{}apikey={}",
            self.base_url, endpoint, separator, self.api_key
        )
    }

    pub async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let url = self.url_with_api_key(endpoint);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(Error::HttpError)?;

        self.handle_response(response).await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.url_with_api_key(endpoint);

        let response = self
            .client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(Error::HttpError)?;

        self.handle_response(response).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let url = self.url_with_api_key(endpoint);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(Error::HttpError)?;

        self.handle_response(response).await
    }

    async fn handle_response<T: DeserializeOwned>(&self, response: Response) -> Result<T> {
        let status = response.status();

        if status.is_success() {
            response.json::<T>().await.map_err(|e| Error::ApiError {
                status: status.as_u16(),
                message: format!("failed to parse response: {}", e),
            })
        } else if status.as_u16() == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1000);

            Err(Error::RateLimitExceeded {
                retry_after_ms: retry_after,
            })
        } else {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());

            Err(Error::ApiError {
                status: status.as_u16(),
                message,
            })
        }
    }
}

impl Clone for RestClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mock_config() -> ApiConfig {
        ApiConfig {
            api_key: "test-api-key".into(),
            rest_base_url: "https://proxy.opinion.trade:8443/openapi".into(),
            timeout: Duration::from_secs(10),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(100),
        }
    }

    #[test]
    fn test_rest_client_creation() {
        let config = mock_config();
        let client = RestClient::new(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_url_with_api_key() {
        let config = mock_config();
        let client = RestClient::new(&config).unwrap();

        let url = client.url_with_api_key("markets");
        assert!(url.contains("apikey=test-api-key"));
        assert!(url.contains("?apikey="));

        let url_with_params = client.url_with_api_key("orderbook?pair=BTC");
        assert!(url_with_params.contains("&apikey="));
    }

    #[test]
    fn test_rest_client_clone() {
        let config = mock_config();
        let client1 = RestClient::new(&config).unwrap();
        let client2 = client1.clone();

        assert_eq!(client1.base_url, client2.base_url);
        assert_eq!(client1.api_key, client2.api_key);
    }
}
