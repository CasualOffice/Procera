//! Which request types a workspace offers, to whom, and in which currency (BPMS-103, docs/20 §2.7a):
//!
//! - `requests.types.enabled`: the built-in types offered (`["*"]` = all, the default);
//! - `requests.types.audience`: per type, who may start it (the campaign/announcement audience
//!   model); a type that is not in the map is open to everyone holding `request:start`;
//! - `requests.currency`: the currency money fields use and approval thresholds read in.
//!
//! The catalogue lists only what the caller may start, and starting anything else is refused
//! (`Forbidden`), so a hidden type cannot be started by guessing its key.

use std::sync::Arc;

use procera_db::WsTx;
use procera_members::audience::{self, Audience};
use procera_settings::Settings;
use serde_json::Value;
use uuid::Uuid;

use crate::catalogue::RequestType;
use crate::{RequestError, Requests};

type R<T> = Result<T, RequestError>;

/// The workspace's request settings, read once per call.
#[derive(Debug, Clone)]
pub struct Offer {
    enabled: Option<Vec<String>>,
    audiences: serde_json::Map<String, Value>,
    pub currency: String,
}

impl Offer {
    pub fn enabled(&self, key: &str) -> bool {
        self.enabled.as_ref().is_none_or(|e| e.iter().any(|k| k == key))
    }

    /// The audience a type is limited to; `None` = everyone.
    pub fn audience(&self, key: &str) -> R<Option<Audience>> {
        match self.audiences.get(key) {
            None => Ok(None),
            Some(v) => {
                let a: Audience = serde_json::from_value(v.clone())?;
                Ok((!a.all).then_some(a))
            }
        }
    }
}

impl Requests {
    fn settings(&self) -> Settings {
        Settings::new(self.clock.clone(), self.ids.clone())
    }

    /// The request settings of the transaction's workspace.
    pub async fn offer(&self, tx: &mut WsTx<'_>) -> R<Offer> {
        let st = self.settings();
        let enabled = st.effective(tx, "requests.types.enabled").await?.value;
        let enabled = match enabled.as_array() {
            Some(items) if items.iter().any(|i| i == "*") => None,
            Some(items) => Some(items.iter().filter_map(Value::as_str).map(str::to_owned).collect()),
            None => None,
        };
        let audiences =
            st.effective(tx, "requests.types.audience").await?.value.as_object().cloned().unwrap_or_default();
        let currency =
            st.effective(tx, "requests.currency").await?.value.as_str().unwrap_or("EUR").to_owned();
        Ok(Offer { enabled, audiences, currency })
    }

    /// Whether `member` may start `key` under `offer`: the type is enabled and they are in its audience.
    pub async fn may_start(&self, tx: &mut WsTx<'_>, offer: &Offer, key: &str, member: Uuid) -> R<bool> {
        if !offer.enabled(key) {
            return Ok(false);
        }
        Ok(match offer.audience(key)? {
            None => true,
            Some(a) => audience::contains(tx, &a, member).await?,
        })
    }

    /// The types `member` may start, in catalogue order.
    pub async fn offered(&self, tx: &mut WsTx<'_>, member: Uuid) -> R<(Offer, Vec<Arc<RequestType>>)> {
        let offer = self.offer(tx).await?;
        let mut out = Vec::new();
        for t in self.catalogue.list() {
            if self.may_start(tx, &offer, t.key, member).await? {
                out.push(t.clone());
            }
        }
        Ok((offer, out))
    }
}
