//! Domain search, purchase, and ownership endpoints for moss-seta.
//!
//! The types these methods speak live in the sibling `wire` module and are
//! re-exported here, so `seta::domains::X` keeps naming them.

use super::client::{MossSetaClient, SetaError};

// A file split, not a second path: `wire` is private and its contents are
// re-exported, so `seta::domains::X` stays the one way to name them.
mod wire;
pub use wire::*;

impl MossSetaClient {
    // ========== Public (Unauthenticated) Domain Endpoints ==========

    /// Search for domain availability (public, no auth required).
    ///
    /// The `phase` parameter controls which TLDs to search:
    /// - `SearchPhase::Popular`: Search 14 popular TLDs (com, net, org, io, etc.)
    /// - `SearchPhase::Extended`: Search 14 extended TLDs (online, site, store, etc.)
    /// - `SearchPhase::All`: Search all 28 TLDs
    ///
    /// This endpoint does not require authentication. It can be called
    /// with either an authenticated or unauthenticated client.
    pub async fn search_domains(
        &self,
        query: &str,
        phase: &SearchPhase,
    ) -> Result<DomainSearchResponse, SetaError> {
        let url = self.secure_url("/api/domains/search");

        let response = self.client
            .get(url)
            .query(&[("q", query), ("phase", &phase.to_string())])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        let mut result: DomainSearchResponse = response.json().await
            .map_err(|e| SetaError::Parse(e.to_string()))?;

        // Fill in defaults for backward compatibility with old API
        if result.phase.is_empty() {
            result.phase = phase.to_string();
        }
        if result.available_count.is_none() {
            result.available_count = Some(result.results.iter().filter(|r| r.available).count() as u32);
        }

        Ok(result)
    }

    /// Search for domain suggestions using NAME_SUGGEST API (public, no auth required).
    ///
    /// Returns both exact-match availability (lookup) and AI-generated
    /// alternatives (suggestions) in a single API call.
    pub async fn suggest_domains(&self, query: &str) -> Result<DomainSuggestResponse, SetaError> {
        let url = self.secure_url("/api/domains/suggest");

        let response = self.client
            .get(url)
            .query(&[("q", query)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Fast domain suggestions - popular TLDs only (~1s response, public, no auth required).
    ///
    /// Returns suggestions for popular TLDs (com, net, org, io, dev, app, ai).
    /// Results are cached for 5 minutes on the server side.
    pub async fn suggest_domains_fast(&self, query: &str) -> Result<DomainSuggestResponse, SetaError> {
        let url = self.secure_url("/api/domains/suggest/fast");

        let response = self.client
            .get(url)
            .query(&[("q", query)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Extended domain suggestions - less common TLDs (~3s response, public, no auth required).
    ///
    /// Returns suggestions for extended TLDs (co, me, xyz, tech, etc.).
    /// Results are cached for 5 minutes on the server side.
    pub async fn suggest_domains_extended(&self, query: &str) -> Result<DomainSuggestResponse, SetaError> {
        let url = self.secure_url("/api/domains/suggest/extended");

        let response = self.client
            .get(url)
            .query(&[("q", query)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Check domain availability using authoritative LOOKUP (public, no auth required).
    ///
    /// This is more reliable than NAME_SUGGEST as it queries the actual registry.
    /// Use this before showing the payment form to ensure the domain is available.
    ///
    /// # Arguments
    /// * `domain` - The full domain name to check (e.g., "example.com")
    pub async fn check_domain_availability(&self, domain: &str) -> Result<DomainAvailabilityResult, SetaError> {
        let url = self.secure_url("/api/domains/check-availability");

        log::debug!(target: "seta", "GET {} domain={}", url, domain);

        let response = self.client
            .get(&url)
            .query(&[("domain", domain)])
            .send()
            .await?;

        log::debug!(target: "seta", "Response status: {}", response.status());

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    // ========== Authenticated Domain Purchase Endpoints ==========

    /// Create a payment intent for domain purchase.
    ///
    /// # Arguments
    /// * `domain` - The domain name to purchase (e.g., "example.com")
    /// * `years` - Number of years to register (1-10)
    pub async fn create_payment_intent(&self, domain: &str, years: i32) -> Result<PaymentIntentResponse, SetaError> {
        self.post_json("/api/domains/create-payment", &serde_json::json!({ "domain": domain, "years": years })).await
    }

    /// Charge a saved payment method for a domain purchase.
    ///
    /// POSTs to `/api/payment/domains/charge-saved`. Backend returns
    /// `{status, payment_intent_id, client_secret, price_cents}` on success.
    /// A failed charge comes back as HTTP 400 and surfaces as `SetaError::Api`.
    ///
    /// **Do NOT retry on network failure.** The backend idempotency key is
    /// bucketed per minute, so a retry from a different tab or session may
    /// collide with a legitimate new attempt. Let the user decide to retry
    /// from the UI.
    //
    // `self.client` retries nothing on its own: `build_seta_http_client` sets
    // timeouts and a proxy, never a retry policy, and `retry_transient` is
    // opt-in per call site.
    pub async fn charge_saved_for_domain(
        &self,
        payment_method_id: &str,
        domain: &str,
        years: i32,
    ) -> Result<ChargeSavedResult, SetaError> {
        debug_assert!(
            (1..=10).contains(&years),
            "years must be 1..=10"
        );

        self.post_json("/api/payment/domains/charge-saved", &serde_json::json!({
            "payment_method_id": payment_method_id,
            "domain": domain,
            "years": years,
        }))
        .await
    }

    /// Fetch billing address for a saved payment method.
    ///
    /// GETs `/api/payment/methods/{pm_id}/billing`. The browser Stripe SDK
    /// has no retrievePaymentMethod API, so after `handleNextAction` resolves
    /// 3DS on a saved card the widget calls through this endpoint to get the
    /// on-file billing address for the registrant contact.
    pub async fn get_payment_method_billing(
        &self,
        payment_method_id: &str,
    ) -> Result<BillingAddress, SetaError> {
        self.get_json(&format!("/api/payment/methods/{}/billing", urlencoding::encode(payment_method_id))).await
    }

    /// List saved payment methods for the authenticated user.
    ///
    /// GETs `/api/payment/methods`. Backend returns
    /// `{ payment_methods: [{ id, brand, last4, exp_month, exp_year }] }`.
    /// Users without a Stripe customer on file receive an empty list (not an
    /// error).
    pub async fn list_payment_methods(&self) -> Result<Vec<StoredPaymentMethodRemote>, SetaError> {
        #[derive(serde::Deserialize)]
        struct Envelope {
            payment_methods: Vec<StoredPaymentMethodRemote>,
        }
        let envelope: Envelope = self.get_json("/api/payment/methods").await?;
        Ok(envelope.payment_methods)
    }

    /// Get user's owned domains.
    ///
    /// Returns all domains associated with the authenticated user's email.
    pub async fn get_domains(&self) -> Result<UserDomainsResponse, SetaError> {
        self.get_json("/api/domains").await
    }

    /// Register a domain after payment is confirmed.
    ///
    /// # Arguments
    /// * `domain` - The domain to register
    /// * `payment_intent_id` - Stripe payment intent ID to verify
    /// * `registrant` - ICANN-required contact information
    pub async fn register_domain(
        &self,
        domain: &str,
        payment_intent_id: &str,
        registrant: &RegistrantContact,
    ) -> Result<DomainRegistrationResponse, SetaError> {
        // The server derives city and state from the payment's postal code
        // and uses the account's verified email, so only what the user typed
        // is sent. It reads the domain from the payment, so `domain` is only
        // logged.
        log::debug!(target: "seta", "POST /api/domains/register domain={}", domain);
        self.post_json("/api/domains/register", &serde_json::json!({
            "payment_intent_id": payment_intent_id,
            "registrant": registrant,
        }))
        .await
    }

    /// Get all domains owned by the authenticated user.
    ///
    /// Calls GET /api/domains/owned and returns both moss-purchased and
    /// externally-linked domains with their current assignment.
    pub async fn get_assignable_domains(&self) -> Result<AssignableDomainsResponse, SetaError> {
        self.get_json("/api/domains/owned").await
    }

    /// Assign (or reassign) an owned domain to a site.
    ///
    /// Calls PUT /api/domains/:host/assign with body `{ folder_id }`.
    /// The domain must already be owned by the authenticated user.
    ///
    /// # Arguments
    /// * `host` - The domain to assign (e.g. "example.com")
    /// * `folder_id` - The site/folder ID to assign the domain to
    pub async fn assign_domain(&self, host: &str, folder_id: &str) -> Result<(), SetaError> {
        let path = format!("/api/domains/{}/assign", urlencoding::encode(host));
        let response = self
            .signed_json(reqwest::Method::PUT, &path, &serde_json::json!({ "folder_id": folder_id }))?
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        Ok(())
    }

    /// Get DNS and TLS certificate status for an owned domain.
    ///
    /// Calls GET /api/domains/:host/status. The domain must be owned by the
    /// authenticated user. Named `get_domain_status_for_host` to avoid
    /// collision with the local `get_domain_status` command that reads
    /// `.moss/config.toml`.
    ///
    /// # Arguments
    /// * `host` - The domain to check (e.g. "example.com")
    pub async fn get_domain_status_for_host(&self, host: &str) -> Result<DomainStatus, SetaError> {
        self.get_json(&format!("/api/domains/{}/status", urlencoding::encode(host))).await
    }
}
