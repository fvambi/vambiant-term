//! The one HTTP client the adapters share: bounded timeouts, error bodies
//! read instead of thrown, never a credential in an error message.

use std::io::Read;
use std::time::Duration;

use crate::provider::ProviderError;

/// A client with one global timeout.
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new(Duration::from_secs(90))
    }
}

impl Http {
    /// A client whose every request (connect, headers, body) must finish
    /// within `timeout`.
    pub fn new(timeout: Duration) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent }
    }

    /// POST JSON; returns the body reader on 2xx, a `Status` error with
    /// the (truncated) body otherwise.
    pub fn post_json(
        &self,
        provider: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &serde_json::Value,
    ) -> Result<Box<dyn Read + Send + Sync + 'static>, ProviderError> {
        let mut req = self.agent.post(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send_json(body).map_err(|e| ProviderError::Transport {
            provider: provider.to_owned(),
            url: url.to_owned(),
            detail: e.to_string(),
        })?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            return Err(ProviderError::Status {
                provider: provider.to_owned(),
                status,
                body: body.chars().take(600).collect(),
            });
        }
        Ok(Box::new(resp.into_body().into_reader()))
    }

    /// GET and parse JSON; non-2xx is a `Status` error with the body.
    pub fn get_json(
        &self,
        provider: &str,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<serde_json::Value, ProviderError> {
        let mut req = self.agent.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.call().map_err(|e| ProviderError::Transport {
            provider: provider.to_owned(),
            url: url.to_owned(),
            detail: e.to_string(),
        })?;
        let status = resp.status().as_u16();
        let text = resp.into_body().read_to_string().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(ProviderError::Status {
                provider: provider.to_owned(),
                status,
                body: text.chars().take(600).collect(),
            });
        }
        serde_json::from_str(&text).map_err(|e| ProviderError::Protocol {
            provider: provider.to_owned(),
            detail: e.to_string(),
        })
    }
}
