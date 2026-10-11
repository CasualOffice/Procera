//! API errors as RFC 9457 problem details (docs/08 §3).

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use procera_identity::IdentityError;
use procera_kernel::Problem;

#[derive(Debug)]
pub struct ApiError {
    pub problem: Box<Problem>,
    pub retry_after: Option<i64>,
}

impl ApiError {
    pub fn new(status: u16, code: &str, title: &str) -> Self {
        Self { problem: Box::new(Problem::new(status, code, title)), retry_after: None }
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.problem = Box::new(self.problem.with_detail(d));
        self
    }
    pub fn unauthenticated() -> Self {
        Self::new(401, "unauthenticated", "Authentication required")
    }
    /// Hidden ≡ missing (docs/12 §1): callers who may not know a resource exists get 404, not 403.
    pub fn not_found() -> Self {
        Self::new(404, "not_found", "Not found")
    }
    pub fn unavailable(what: &str) -> Self {
        Self::new(503, "unavailable", "Service unavailable").detail(format!("{what} is not configured"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut res = (status, [(header::CONTENT_TYPE, "application/problem+json")], Json(self.problem))
            .into_response();
        if let Some(s) = self.retry_after
            && let Ok(v) = HeaderValue::from_str(&s.to_string())
        {
            res.headers_mut().insert(header::RETRY_AFTER, v);
        }
        if status == StatusCode::UNAUTHORIZED {
            res.headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer realm=\"procera\""));
        }
        res
    }
}

impl From<IdentityError> for ApiError {
    fn from(e: IdentityError) -> Self {
        match e {
            IdentityError::InvalidCredentials => {
                Self::new(401, "invalid_credentials", "Invalid email or password")
            }
            IdentityError::Throttled { retry_after_secs } => {
                let mut a = Self::new(429, "rate_limited", "Too many attempts")
                    .detail(format!("Try again in {retry_after_secs} seconds"));
                a.retry_after = Some(retry_after_secs);
                a
            }
            IdentityError::Policy(p) => {
                Self::new(422, "password_policy", "Password does not meet the policy").detail(p.to_string())
            }
            IdentityError::EmailTaken => {
                Self::new(409, "email_taken", "An account with this email already exists")
            }
            IdentityError::InvalidCode => Self::new(401, "invalid_code", "Invalid or expired code"),
            IdentityError::InvalidToken => Self::new(401, "invalid_token", "Invalid or expired token"),
            IdentityError::RefreshReused => Self::new(401, "token_reused", "Session revoked")
                .detail("A refresh token was used twice; sign in again"),
            IdentityError::NoPendingEnrolment => {
                Self::new(409, "no_pending_enrolment", "Start MFA enrolment first")
            }
            IdentityError::PasskeyRejected(reason) => {
                Self::new(401, "passkey_rejected", "The passkey could not be verified").detail(reason)
            }
            IdentityError::PasskeyInvalid(reason) => {
                Self::new(422, "passkey_invalid", "The passkey could not be registered").detail(reason)
            }
            IdentityError::PasskeyExists => {
                Self::new(409, "passkey_exists", "This passkey is already registered")
            }
            IdentityError::NotFound => Self::not_found(),
            IdentityError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            IdentityError::Internal(msg) => {
                tracing::error!(error = %msg, "identity internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}
