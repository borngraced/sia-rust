use crate::http::endpoints::{AddressBalanceRequest, AddressBalanceResponse, ConsensusTipRequest, SiaApiRequest};
use crate::types::Address;
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use http::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client as ReqwestClient;
use url::Url;

use crate::http::client::{ApiClient, ApiClientError, ApiClientHelpers, ClientConf, EndpointSchema};
use core::time::Duration;

#[derive(Clone)]
pub struct NativeClient {
    pub client: ReqwestClient,
    pub base_url: Url,
}

#[async_trait]
impl ApiClient for NativeClient {
    type Request = reqwest::Request;
    type Response = reqwest::Response;

    async fn new(conf: ClientConf) -> Result<Self, ApiClientError> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Basic {}", BASE64.encode(format!(":{}", conf.password)));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).map_err(|e| ApiClientError::BuildError(e.to_string()))?,
        );

        let timeout = conf.timeout.unwrap_or(10);
        let client = ReqwestClient::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(timeout))
            .build()
            .map_err(ApiClientError::ReqwestError)?;

        let ret = NativeClient {
            client,
            base_url: conf.url,
        };
        // Ping the server with ConsensusTipRequest to check if the client is working
        ret.dispatcher(ConsensusTipRequest).await?;
        Ok(ret)
    }

    fn process_schema(&self, schema: EndpointSchema) -> Result<Self::Request, ApiClientError> {
        let url = schema.build_url(&self.base_url)?;
        Ok(Self::Request::new(schema.method, url))
    }

    fn to_data_request<R: SiaApiRequest>(&self, request: R) -> Result<Self::Request, ApiClientError> {
        let schema = request.to_endpoint_schema()?;
        self.process_schema(schema)
    }

    async fn execute_request(&self, request: Self::Request) -> Result<Self::Response, ApiClientError> {
        self.client.execute(request).await.map_err(ApiClientError::ReqwestError)
    }

    async fn dispatcher<R: SiaApiRequest>(&self, request: R) -> Result<R::Response, ApiClientError> {
        let request = self.to_data_request(request)?;

        // Execute the request using reqwest client
        let response = self
            .client
            .execute(request)
            .await
            .map_err(ApiClientError::ReqwestError)?;

        // Check the response status and return the appropriate result
        match response.status() {
            reqwest::StatusCode::OK => Ok(response
                .json::<R::Response>()
                .await
                .map_err(ApiClientError::ReqwestError)?),
            reqwest::StatusCode::NO_CONTENT => {
                if let Some(resp_type) = R::is_empty_response() {
                    Ok(resp_type)
                } else {
                    Err(ApiClientError::UnexpectedEmptyResponse {
                        expected_type: std::any::type_name::<R::Response>().to_string(),
                    })
                }
            },
            _ => Err(ApiClientError::UnexpectedHttpStatus(response.status())),
        }
    }
}

#[async_trait]
impl ApiClientHelpers for NativeClient {
    async fn current_height(&self) -> Result<u64, ApiClientError> {
        Ok(self.dispatcher(ConsensusTipRequest).await?.height)
    }

    async fn address_balance(&self, address: Address) -> Result<AddressBalanceResponse, ApiClientError> {
        self.dispatcher(AddressBalanceRequest { address }).await
    }
}

// TODO these tests should not rely on the actual server - mock the server or use docker
#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::endpoints::{AddressBalanceRequest, GetEventRequest};

    use std::str::FromStr;
    use tokio;

    async fn init_client() -> NativeClient {
        let conf = ClientConf {
            url: Url::parse("https://sia-walletd.komodo.earth/").unwrap(),
            password: "password".to_string(),
            timeout: Some(10),
        };
        NativeClient::new(conf).await.unwrap()
    }

    /// Helper function to setup the client and send a request
    async fn test_dispatch<R: SiaApiRequest>(request: R) -> R::Response {
        let api_client = init_client().await;
        api_client.dispatcher(request).await.unwrap()
    }

    #[tokio::test]
    async fn test_new_client() { let _api_client = init_client().await; }

    #[tokio::test]
    async fn test_api_consensus_tip() {
        // paranoid unit test - NativeClient::new already pings the server with ConsensusTipRequest
        let _response = test_dispatch(ConsensusTipRequest).await;
    }

    #[tokio::test]
    async fn test_api_address_balance() {
        let request = AddressBalanceRequest {
            address: Address::from_str(
                "addr:591fcf237f8854b5653d1ac84ae4c107b37f148c3c7b413f292d48db0c25a8840be0653e411f",
            )
            .unwrap(),
        };
        let _response = test_dispatch(request).await;
    }

    #[tokio::test]
    async fn test_api_events() {
        use crate::types::H256;
        let request = GetEventRequest {
            txid: H256::from_str("77c5ae2220eac76dd841e365bb14fcba5499977e6483472b96f4a83bcdd6c892").unwrap(),
        };
        let _response = test_dispatch(request).await;
    }
}
