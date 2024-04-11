use std::convert::Infallible;

use async_trait::async_trait;
use log::{error, trace, warn};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::from_str;

use crate::errors::{Error, MeilisearchCommunicationError, MeilisearchError};

#[derive(Debug)]
pub enum Method<Q, B> {
    Get { query: Q },
    Post { query: Q, body: B },
    Patch { query: Q, body: B },
    Put { query: Q, body: B },
    Delete { query: Q },
}

#[async_trait(?Send)]
pub trait HttpClient: Clone + Send + Sync {
    async fn request<Query, Body, Output>(
        self,
        url: &str,
        apikey: Option<&str>,
        method: Method<Query, Body>,
        expected_status_code: u16,
    ) -> Result<Output, Error>
    where
        Query: Serialize + Send + Sync,
        Body: Serialize + Send + Sync,
        Output: DeserializeOwned + 'static + Send;

    async fn stream_request<
        'a,
        Query: Serialize + Send + Sync,
        Body: futures_io::AsyncRead + Send + Sync + 'static,
        Output: DeserializeOwned + 'static,
    >(
        self,
        url: &str,
        apikey: Option<&str>,
        method: Method<Query, Body>,
        content_type: &str,
        expected_status_code: u16,
    ) -> Result<Output, Error>;
}

#[cfg(feature = "reqwest")]
#[derive(Debug, Clone, Copy, Default)]
pub struct ReqwestClient;

#[cfg(feature = "reqwest")]
impl ReqwestClient {
    pub fn new() -> Self {
        ReqwestClient
    }
}

#[cfg(feature = "reqwest")]
#[async_trait(?Send)]
impl HttpClient for ReqwestClient {
    async fn request<Query, Body, Output>(
        self,
        url: &str,
        apikey: Option<&str>,
        method: Method<Query, Body>,
        expected_status_code: u16,
    ) -> Result<Output, Error>
    where
        Query: Serialize + Send + Sync,
        Body: Serialize + Send + Sync,
        Output: DeserializeOwned + 'static + Send,
    {
        use reqwest::{header, ClientBuilder};
        use serde_json::to_string;

        let builder = ClientBuilder::new();
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&qualified_version()).unwrap(),
        );

        if let Some(apikey) = apikey {
            headers.insert(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&format!("Bearer {apikey}")).unwrap(),
            );
        }

        let builder = builder.default_headers(headers);
        let client = builder.build().unwrap();

        let request = match &method {
            Method::Get { query } => {
                let url = add_query_parameters(url, query)?;

                client.request(reqwest::Method::GET, url.as_str()).build()?
            }
            Method::Delete { query } => {
                let url = add_query_parameters(url, query)?;

                client
                    .request(reqwest::Method::DELETE, url.as_str())
                    .build()?
            }
            Method::Post { query, body } => {
                let url = add_query_parameters(url, query)?;

                client
                    .request(reqwest::Method::POST, url.as_str())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(to_string(&body).unwrap())
                    .build()?
            }
            Method::Patch { query, body } => {
                let url = add_query_parameters(url, query)?;

                client
                    .request(reqwest::Method::PATCH, url.as_str())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(to_string(&body).unwrap())
                    .build()?
            }
            Method::Put { query, body } => {
                let url = add_query_parameters(url, query)?;

                client
                    .request(reqwest::Method::PUT, url.as_str())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(to_string(&body).unwrap())
                    .build()?
            }
        };

        let response = client.execute(request).await.unwrap();

        let status = response.status().as_u16();

        let mut body = response.text().await?;

        if body.is_empty() {
            body = "null".to_string();
        }

        parse_response(status, expected_status_code, &body, url.to_string())
    }

    async fn stream_request<
        'a,
        Query: Serialize + Send + Sync,
        Body: futures_io::AsyncRead + Send + Sync + 'static,
        Output: DeserializeOwned + 'static,
    >(
        self,
        url: &str,
        apikey: Option<&str>,
        method: Method<Query, Body>,
        content_type: &str,
        expected_status_code: u16,
    ) -> Result<Output, Error> {
        use reqwest::{header, ClientBuilder};

        let builder = ClientBuilder::new();
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&qualified_version()).unwrap(),
        );

        if let Some(apikey) = apikey {
            headers.insert(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&format!("Bearer {apikey}")).unwrap(),
            );
        }

        let builder = builder.default_headers(headers);
        let client = builder.build().unwrap();

        let request = match method {
            Method::Get { query } => {
                let url = add_query_parameters(url, &query)?;

                client.request(reqwest::Method::GET, url.as_str()).build()?
            }
            Method::Delete { query } => {
                let url = add_query_parameters(url, &query)?;

                client
                    .request(reqwest::Method::DELETE, url.as_str())
                    .build()?
            }
            Method::Post { query, body } => {
                let url = add_query_parameters(url, &query)?;

                let reader = tokio_util::compat::FuturesAsyncReadCompatExt::compat(body);
                let stream = tokio_util::io::ReaderStream::new(reader);
                let body = reqwest::Body::wrap_stream(stream);

                client
                    .request(reqwest::Method::POST, url.as_str())
                    .header(header::CONTENT_TYPE, content_type)
                    .body(body)
                    .build()?
            }
            Method::Patch { query, body } => {
                let url = add_query_parameters(url, &query)?;

                let reader = tokio_util::compat::FuturesAsyncReadCompatExt::compat(body);
                let stream = tokio_util::io::ReaderStream::new(reader);
                let body = reqwest::Body::wrap_stream(stream);

                client
                    .request(reqwest::Method::PATCH, url.as_str())
                    .header(header::CONTENT_TYPE, content_type)
                    .body(body)
                    .build()?
            }
            Method::Put { query, body } => {
                let url = add_query_parameters(url, &query)?;

                let reader = tokio_util::compat::FuturesAsyncReadCompatExt::compat(body);
                let stream = tokio_util::io::ReaderStream::new(reader);
                let body = reqwest::Body::wrap_stream(stream);

                client
                    .request(reqwest::Method::PUT, url.as_str())
                    .header(header::CONTENT_TYPE, content_type)
                    .body(body)
                    .build()?
            }
        };

        let response = client.execute(request).await.unwrap();

        let status = response.status().as_u16();

        let mut body = response.text().await?;

        if body.is_empty() {
            body = "null".to_string();
        }

        parse_response(status, expected_status_code, &body, url.to_string())
    }
}

pub fn add_query_parameters<Query: Serialize>(url: &str, query: &Query) -> Result<String, Error> {
    let query = yaup::to_string(query)?;

    if query.is_empty() {
        Ok(url.to_string())
    } else {
        Ok(format!("{url}?{query}"))
    }
}

pub fn parse_response<Output: DeserializeOwned>(
    status_code: u16,
    expected_status_code: u16,
    body: &str,
    url: String,
) -> Result<Output, Error> {
    if status_code == expected_status_code {
        return match from_str::<Output>(body) {
            Ok(output) => {
                trace!("Request succeed");
                Ok(output)
            }
            Err(e) => {
                error!("Request succeeded but failed to parse response");
                Err(Error::ParseError(e))
            }
        };
    }

    warn!(
        "Expected response code {}, got {}",
        expected_status_code, status_code
    );

    match from_str::<MeilisearchError>(body) {
        Ok(e) => Err(Error::from(e)),
        Err(e) => {
            if status_code >= 400 {
                return Err(Error::MeilisearchCommunication(
                    MeilisearchCommunicationError {
                        status_code,
                        message: None,
                        url,
                    },
                ));
            }
            Err(Error::ParseError(e))
        }
    }
}

pub fn qualified_version() -> String {
    const VERSION: Option<&str> = option_env!("CARGO_PKG_VERSION");

    format!("Meilisearch Rust (v{})", VERSION.unwrap_or("unknown"))
}

#[async_trait(?Send)]
impl HttpClient for Infallible {
    async fn request<Query, Body, Output>(
        self,
        _url: &str,
        _apikey: Option<&str>,
        _method: Method<Query, Body>,
        _expected_status_code: u16,
    ) -> Result<Output, Error>
    where
        Query: Serialize + Send + Sync,
        Body: Serialize + Send + Sync,
        Output: DeserializeOwned + 'static + Send,
    {
        unreachable!()
    }

    async fn stream_request<
        'a,
        Query: Serialize + Send + Sync,
        Body: futures_io::AsyncRead + Send + Sync + 'static,
        Output: DeserializeOwned + 'static,
    >(
        self,
        _url: &str,
        _apikey: Option<&str>,
        _method: Method<Query, Body>,
        _content_type: &str,
        _expected_status_code: u16,
    ) -> Result<Output, Error> {
        unreachable!()
    }
}
