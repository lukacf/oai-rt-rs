use crate::protocol::models::{SessionConfig};
use crate::transport::rest::{CallCreationResponse, EphemeralSecretResponse, ExpiresAfter, RealtimeRestAdapter};
use crate::Result;

/// High-level REST helper for WebRTC/SIP call control.
#[derive(Clone, Debug)]
pub struct Calls {
    rest: RealtimeRestAdapter,
}

impl Calls {
    /// # Errors
    /// Returns an error if the API key is invalid or the HTTP client cannot be built.
    #[allow(clippy::result_large_err)]
    pub fn new(api_key: &str) -> Result<Self> {
        Ok(Self {
            rest: RealtimeRestAdapter::new(api_key)?,
        })
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_client_secret(&self, session: SessionConfig) -> Result<EphemeralSecretResponse> {
        self.rest.create_client_secret(session).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_client_secret_with_expiry(
        &self,
        session: SessionConfig,
        expires_after: ExpiresAfter,
    ) -> Result<EphemeralSecretResponse> {
        self.rest.create_client_secret_with_expiry(session, Some(expires_after)).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn webrtc_offer_raw(&self, sdp_offer: String) -> Result<String> {
        self.rest.post_sdp_offer_raw(sdp_offer).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn webrtc_offer_raw_with_call_id(&self, sdp_offer: String) -> Result<CallCreationResponse> {
        self.rest.post_sdp_offer_raw_with_call_id(sdp_offer).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn webrtc_offer_multipart(
        &self,
        sdp_offer: String,
        session: Option<SessionConfig>,
    ) -> Result<String> {
        self.rest.post_sdp_offer_multipart(sdp_offer, session).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn webrtc_offer_multipart_with_call_id(
        &self,
        sdp_offer: String,
        session: Option<SessionConfig>,
    ) -> Result<CallCreationResponse> {
        self.rest.post_sdp_offer_multipart_with_call_id(sdp_offer, session).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn sip_accept(&self, call_id: &str, session: SessionConfig) -> Result<()> {
        self.rest.sip_accept(call_id, session).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn sip_reject(&self, call_id: &str) -> Result<()> {
        self.rest.sip_reject(call_id).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn hangup(&self, call_id: &str) -> Result<()> {
        self.rest.hangup(call_id).await
    }

    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn sip_refer(&self, call_id: &str, target_uri: impl Into<String>) -> Result<()> {
        self.rest.sip_refer(call_id, target_uri).await
    }
}
