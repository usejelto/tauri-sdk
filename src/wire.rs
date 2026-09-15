use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, str::FromStr};

pub const MAX_BYTES: usize = 65_536;
pub const QUEUE_BYTES: usize = 1_048_576;
pub const QUEUE_EVENTS: usize = 1_000;
pub const DAY: u64 = 86_400_000;
pub const CLAIM_AFTER: u64 = 30 * DAY;
pub const BACKOFF_MAX: u64 = 3_600_000;

/// JSON scalar properties. Install properties accept only wire §4's string grammar.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropValue {
    String(String),
    Number(serde_json::Number),
    Boolean(bool),
}
pub type Props = BTreeMap<String, PropValue>;

pub(crate) fn log(debug: bool, message: &str) {
    if debug {
        // Never format transport errors: those can include an endpoint's IP address.
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "jelto: {message}");
    }
}

pub(crate) fn grammar(s: &str, min: usize, max: usize, extra: &str) -> bool {
    (min..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.as_bytes().contains(&b))
}

pub(crate) fn valid_props(props: &Props, debug: bool) -> bool {
    let reason = if props.len() > 20 {
        Some("props: spec/wire-v1.md §3 caps them at 20")
    } else if props.keys().any(|key| !grammar(key, 1, 32, "_")) {
        Some("props keys must match ^[a-z0-9_]{1,32}$")
    } else if props.values().any(|v| match v {
        PropValue::String(s) => s.chars().count() > 200,
        PropValue::Number(n) => n.to_string().chars().count() > 200,
        PropValue::Boolean(_) => false,
    }) {
        Some("props: spec/wire-v1.md §3 caps a string at 200")
    } else {
        None
    };
    if let Some(reason) = reason {
        log(debug, &format!("drop event: {reason}"));
    }
    reason.is_none()
}

pub(crate) fn known_app_version(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 32
}

pub(crate) fn client_version(raw: Option<String>, debug: bool) -> Option<String> {
    let value = raw.unwrap_or_else(|| concat!("tauri/", env!("CARGO_PKG_VERSION")).into());
    if value.is_empty() {
        return None;
    }
    let valid = value.split_once('/').is_some_and(|(a, b)| {
        !a.is_empty()
            && a.bytes().all(|c| c.is_ascii_lowercase())
            && (1..=24).contains(&b.len())
            && b.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".+-".contains(&c))
    }) && value.len() <= 32;
    if valid {
        Some(value)
    } else {
        log(
            debug,
            "client version does not match ^[a-z]+/[0-9A-Za-z.+-]{1,24}$; v omitted",
        );
        None
    }
}

/// Parse exact whole JSON numbers without passing through a float, including exponents.
pub(crate) fn whole(raw: &str) -> Option<BigInt> {
    let (mantissa, exponent) = match raw.find(['e', 'E']) {
        Some(i) => (&raw[..i], raw[i + 1..].parse::<i32>().ok()?),
        None => (raw, 0),
    };
    // Bound malicious response exponent expansion; conformance clocks use decimal digits.
    if exponent.unsigned_abs() > 4096 || raw.len() > 8192 {
        return None;
    }
    let decimals = mantissa.split_once('.').map_or(0, |(_, part)| part.len()) as i32;
    let digits = mantissa.replace('.', "");
    let unsigned = digits.strip_prefix('-').unwrap_or(&digits);
    if unsigned.is_empty() || !unsigned.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut value = BigInt::from_str(&digits).ok()?;
    let scale = exponent - decimals;
    let factor = BigInt::from(10).pow(scale.unsigned_abs());
    if scale < 0 {
        if &value % &factor != BigInt::from(0) {
            return None;
        }
        value /= factor;
    } else {
        value *= factor;
    }
    Some(value)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Event {
    pub id: String,
    pub n: String,
    pub t: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: Props,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hb: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<EventMetadata>,
}
impl Event {
    pub fn new(name: &str, now: &BigInt, props: Props, hb: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            n: name.into(),
            t: now.to_string(),
            props,
            hb,
            metadata: None,
        }
    }
    pub fn valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.id).is_ok_and(|id| !id.is_nil())
            && grammar(&self.n, 1, 64, "_:.-")
            && whole(&self.t).is_some()
            && valid_props(&self.props, false)
            && self.metadata.as_ref().is_none_or(EventMetadata::valid)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct EventMetadata {
    av: String,
    os: String,
    osv: String,
    arch: String,
    app: Option<String>,
    version: Option<String>,
}
impl EventMetadata {
    fn valid(&self) -> bool {
        known_app_version(&self.av)
            && matches!(self.os.as_str(), "macos" | "windows" | "linux")
            && matches!(self.arch.as_str(), "arm64" | "x64" | "x86")
            && self.osv.chars().count() <= 32
            && self.app.as_ref().is_none_or(|s| grammar(s, 1, 32, "-"))
            && self
                .version
                .as_ref()
                .is_none_or(|v| client_version(Some(v.clone()), false).as_ref() == Some(v))
    }
}

pub(crate) struct Metadata {
    pub av: String,
    pub os: &'static str,
    pub osv: String,
    pub arch: &'static str,
    pub version: Option<String>,
}
impl Metadata {
    pub fn detect(av: &str, version: Option<String>) -> Self {
        let wire_av: String = av.chars().take(32).collect();
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            "x86" => "x86",
            _ => "",
        };
        let os = match std::env::consts::OS {
            "macos" => "macos",
            "windows" => "windows",
            "linux" => "linux",
            _ => "",
        };
        Self {
            av: if wire_av.trim().is_empty() {
                "1.0.0".into()
            } else {
                wire_av
            },
            os,
            arch,
            osv: os_info::get()
                .version()
                .to_string()
                .chars()
                .take(32)
                .collect(),
            version,
        }
    }
    pub fn snapshot(&self, app: &Option<String>) -> EventMetadata {
        EventMetadata {
            av: self.av.clone(),
            os: self.os.into(),
            osv: self.osv.clone(),
            arch: self.arch.into(),
            app: app.clone(),
            version: self.version.clone(),
        }
    }
    pub fn render(
        &self,
        event: &Event,
        iid: &str,
        app: &Option<String>,
        props: &BTreeMap<String, String>,
    ) -> Option<String> {
        let fallback = self.snapshot(app);
        let metadata = event.metadata.as_ref().unwrap_or(&fallback);
        if !metadata.valid() {
            return None;
        }
        let mut value = json!({"id":event.id,"n":event.n,"t":serde_json::Number::from_str(&event.t).ok()?,
            "s":"app","iid":iid,"av":metadata.av,"os":metadata.os,"osv":metadata.osv,"arch":metadata.arch});
        if let Some(app) = &metadata.app {
            value["a"] = json!(app);
        }
        if let Some(version) = &metadata.version {
            value["v"] = json!(version);
        }
        if event.hb {
            if !props.is_empty() {
                value["props"] = json!(props);
            }
        } else if !event.props.is_empty() {
            value["props"] = json!(event.props);
        }
        serde_json::to_string(&value).ok()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Batch {
    pub body: String,
    pub ids: Vec<String>,
    pub install: bool,
    pub probe: bool,
}

pub(crate) struct Outcome {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: Value,
}
impl Outcome {
    pub fn retryable(&self) -> bool {
        matches!(self.status, 0 | 429 | 503)
    }
}

pub(crate) async fn post(
    client: Option<reqwest::Client>,
    endpoint: String,
    mock: Option<String>,
    body: String,
    debug: bool,
) -> Outcome {
    let failure = || Outcome {
        status: 0,
        retry_after: None,
        body: Value::Null,
    };
    let Some(client) = client else {
        return failure();
    };
    let mut request = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .body(body);
    if let Some(mock) = mock {
        match reqwest::header::HeaderValue::from_str(&mock) {
            Ok(value) => request = request.header("X-Mock", value),
            Err(_) => log(debug, "JELTO_MOCK is not a valid header value; omitted"),
        }
    }
    let Ok(mut response) = request.send().await else {
        return failure();
    };
    let mut outcome = Outcome {
        status: response.status().as_u16(),
        retry_after: response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        body: Value::Null,
    };
    let mut bytes = Vec::new();
    // Receipt of a status is final even if reading its body fails or exceeds the cap.
    while let Ok(Some(chunk)) = response.chunk().await {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return outcome;
        }
        bytes.extend_from_slice(&chunk);
    }
    outcome.body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    outcome
}

pub(crate) fn backoff(step: u64, header: Option<&str>, debug: bool) -> (u64, u64) {
    let step = if step == 0 {
        1000
    } else {
        step.min(BACKOFF_MAX)
    };
    let jittered = (step as f64 * rand::random_range(0.8..=1.2)).round() as u64;
    let mut wait = jittered.clamp(1, BACKOFF_MAX);
    if let Some(raw) = header
        .map(|s| s.trim_matches([' ', '\t']))
        .filter(|s| !s.is_empty())
    {
        if raw.bytes().all(|c| c.is_ascii_digit()) {
            let seconds = BigInt::from_str(raw).unwrap_or_default();
            if seconds > BigInt::from(3600) {
                log(debug, "Retry-After exceeds the 3600 s ceiling; clamped");
            }
            wait = wait.max(seconds.to_u64().unwrap_or(3600).min(3600) * 1000);
        } else {
            log(
                debug,
                "Retry-After is not delay-seconds and is treated as absent",
            );
        }
    }
    (wait, (step * 2).min(BACKOFF_MAX))
}
