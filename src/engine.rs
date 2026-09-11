use crate::{store::Store, wire::*};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::ToPrimitive;
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

type Reply = oneshot::Sender<serde_json::Value>;
enum Command {
    Init(String, Option<String>, Option<String>),
    Track(String, Props),
    Onboarding(String, String, Option<String>),
    SetProps(Props),
    InstallId,
    Reset,
    Disable,
    Flush,
    #[cfg(any(test, feature = "conformance"))]
    Export,
    #[cfg(any(test, feature = "conformance"))]
    LegacyVersion,
    #[cfg(any(test, feature = "conformance"))]
    Advance(u64),
    #[cfg(any(test, feature = "conformance"))]
    Settle,
    #[cfg(any(test, feature = "conformance"))]
    Quit,
}

/// Shared asynchronous SDK handle. Obtain it with `JeltoExt::jelto()`.
/// All I/O and mutable state belong to its single background worker.
#[derive(Clone)]
pub struct Jelto {
    sender: mpsc::Sender<(Command, Option<Reply>)>,
}
impl Jelto {
    #[cfg(feature = "tauri")]
    pub(crate) fn unavailable() -> Self {
        let (sender, _) = mpsc::channel(1);
        Self { sender }
    }
    pub(crate) fn new(dir: Option<PathBuf>, version: String) -> Self {
        let options = Options::environment(dir, version);
        Self::start(options)
    }
    fn start(options: Options) -> Self {
        // Bounded mailbox applies backpressure to concurrent calls from many windows.
        let (sender, receiver) = mpsc::channel(32);
        let debug = options.debug;
        let spawned = std::thread::Builder::new()
            .name("jelto".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    log(debug, "background runtime unavailable");
                    return;
                };
                runtime.block_on(Worker::new(options).run(receiver));
            });
        if spawned.is_err() {
            log(debug, "background worker unavailable");
        }
        Self { sender }
    }
    async fn call(&self, command: Command) -> serde_json::Value {
        let (tx, rx) = oneshot::channel();
        if self.sender.send((command, Some(tx))).await.is_err() {
            return serde_json::Value::Null;
        }
        rx.await.unwrap_or_default()
    }
    /// Start once after consent. Explicit endpoint > JELTO_ENDPOINT > production.
    /// Returns after scheduling initialization; it does not wait for disk or HTTP.
    pub async fn init(&self, key: &str, app: Option<&str>, endpoint: Option<&str>) {
        let _ = self
            .sender
            .send((
                Command::Init(
                    key.into(),
                    app.map(str::to_owned),
                    endpoint.map(str::to_owned),
                ),
                None,
            ))
            .await;
    }
    pub async fn track(&self, name: &str, props: Option<Props>) {
        self.call(Command::Track(name.into(), props.unwrap_or_default()))
            .await;
    }
    pub async fn onboarding(&self, step: &str, status: &str, reason: Option<&str>) {
        self.call(Command::Onboarding(
            step.into(),
            status.into(),
            reason.map(str::to_owned),
        ))
        .await;
    }
    pub async fn set_props(&self, props: Props) {
        self.call(Command::SetProps(props)).await;
    }
    pub async fn install_id(&self) -> String {
        self.call(Command::InstallId)
            .await
            .as_str()
            .unwrap_or_default()
            .into()
    }
    pub async fn reset(&self) {
        self.call(Command::Reset).await;
    }
    pub async fn disable(&self) {
        self.call(Command::Disable).await;
    }
    #[cfg(feature = "tauri")]
    pub(crate) fn background(&self) {
        let _ = self.sender.try_send((Command::Flush, None));
    }

    #[cfg(feature = "conformance")]
    #[doc(hidden)]
    pub fn conformance() -> Result<Self, &'static str> {
        if let Ok(raw) = std::env::var("JELTO_NOW") {
            if whole(&raw).is_none() {
                return Err("JELTO_NOW must be whole milliseconds");
            }
        }
        Ok(Self::new(
            None,
            std::env::var("JELTO_APP_VERSION").unwrap_or_else(|_| "1.0.0".into()),
        ))
    }
    #[cfg(any(test, feature = "conformance"))]
    #[doc(hidden)]
    pub async fn export_state(&self) -> serde_json::Value {
        self.call(Command::Export).await
    }
    #[cfg(any(test, feature = "conformance"))]
    #[doc(hidden)]
    pub async fn legacy_version(&self) -> bool {
        self.call(Command::LegacyVersion)
            .await
            .as_bool()
            .unwrap_or(false)
    }
    #[cfg(any(test, feature = "conformance"))]
    #[doc(hidden)]
    pub async fn advance(&self, millis: u64) {
        self.call(Command::Settle).await;
        self.call(Command::Advance(millis)).await;
    }
    #[cfg(any(test, feature = "conformance"))]
    #[doc(hidden)]
    pub async fn stop(&self) {
        self.call(Command::Flush).await;
        let _ = tokio::time::timeout(Duration::from_millis(600), self.call(Command::Settle)).await;
        self.call(Command::Quit).await;
    }
}

pub(crate) struct Options {
    pub dir: Option<PathBuf>,
    pub av: String,
    pub now: Option<BigInt>,
    pub endpoint: String,
    pub debug: bool,
    pub version: Option<String>,
    pub mock: Option<String>,
}
impl Options {
    fn environment(dir: Option<PathBuf>, av: String) -> Self {
        Self {
            dir: std::env::var_os("JELTO_STATE_DIR")
                .map(PathBuf::from)
                .or(dir),
            av,
            now: harness_now(),
            // An empty JELTO_ENDPOINT is absent (the production default applies).
            // A non-empty value, valid or not, is validated at initialize() time
            // so an invalid override leaves the SDK inactive rather than falling
            // through (spec/wire-v1.md §1).
            endpoint: std::env::var("JELTO_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "https://in.jelto.io/v1/e".into()),
            debug: std::env::var("JELTO_DEBUG").is_ok_and(|s| s == "1"),
            version: harness_client_version(),
            mock: harness_mock(),
        }
    }
}
// These three overrides exist only for the conformance harness and tests; a
// production build must not let the environment steer the clock, client
// version string or mock header.
#[cfg(any(test, feature = "conformance"))]
fn harness_now() -> Option<BigInt> {
    std::env::var("JELTO_NOW").ok().and_then(|s| whole(&s))
}
#[cfg(not(any(test, feature = "conformance")))]
fn harness_now() -> Option<BigInt> {
    None
}
#[cfg(any(test, feature = "conformance"))]
fn harness_client_version() -> Option<String> {
    std::env::var("JELTO_CLIENT_VERSION").ok()
}
#[cfg(not(any(test, feature = "conformance")))]
fn harness_client_version() -> Option<String> {
    None
}
#[cfg(any(test, feature = "conformance"))]
fn harness_mock() -> Option<String> {
    std::env::var("JELTO_MOCK").ok()
}
#[cfg(not(any(test, feature = "conformance")))]
fn harness_mock() -> Option<String> {
    None
}

/// spec/wire-v1.md §1: an endpoint must be an absolute http(s) URL with no userinfo.
fn valid_endpoint(s: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(s) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

/// `Metadata::detect` maps an unrecognized OS or CPU architecture to "".
/// Mirrors the desktop SDKs: an unsupported platform never activates.
fn supported(metadata: &Metadata) -> bool {
    !metadata.os.is_empty() && !metadata.arch.is_empty()
}

struct Worker {
    options: Options,
    store: Store,
    active: bool,
    metadata: Option<Metadata>,
    client: Option<reqwest::Client>,
    endpoint: String,
    init_flush: Option<BigInt>,
    track_flush: Option<BigInt>,
    pending: bool,
    request: Option<Flight>,
    waiters: Vec<Reply>,
    install_enqueued: bool,
}
struct Flight {
    task: JoinHandle<Outcome>,
    ids: Vec<String>,
    install: bool,
    probe: bool,
}
impl Worker {
    fn new(options: Options) -> Self {
        Self {
            store: Store::new(options.dir.clone(), options.debug),
            options,
            active: false,
            metadata: None,
            client: None,
            endpoint: String::new(),
            init_flush: None,
            track_flush: None,
            pending: false,
            request: None,
            waiters: Vec::new(),
            install_enqueued: false,
        }
    }
    fn now(&self) -> BigInt {
        self.options.now.clone().unwrap_or_else(|| {
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(d) => BigInt::from(d.as_millis()),
                Err(e) => -BigInt::from(e.duration().as_millis()),
            }
        })
    }
    async fn run(mut self, mut receiver: mpsc::Receiver<(Command, Option<Reply>)>) {
        loop {
            let now = self.now();
            self.step(&now);
            if self.request.is_none() {
                for waiter in self.waiters.drain(..) {
                    let _ = waiter.send(serde_json::Value::Null);
                }
            }
            let wait = if self.options.now.is_some() {
                Duration::from_secs(86400)
            } else {
                self.next_wait(&now)
            };
            tokio::select! {
                message = receiver.recv() => {
                    let Some((command, reply)) = message else { break; };
                    if !self.command(command, reply).await { break; }
                }
                outcome = async { match self.request.as_mut() {
                    Some(request) => (&mut request.task).await.ok(), None => std::future::pending().await,
                }} => {
                    let request = self.request.take();
                    if let (Some(request), Some(outcome)) = (request, outcome) { self.answered(request, outcome); }
                }
                _ = tokio::time::sleep(wait) => {}
            }
        }
        self.cancel_request();
    }
    fn cancel_request(&mut self) {
        if let Some(request) = self.request.take() {
            request.task.abort();
        }
    }
    async fn command(&mut self, command: Command, reply: Option<Reply>) -> bool {
        let mut result = serde_json::Value::Null;
        match command {
            Command::Init(key, app, endpoint) => self.initialize(key, app, endpoint),
            Command::Track(name, props) if self.active => self.track(&name, props),
            Command::Onboarding(step, status, reason) if self.active => {
                if !grammar(&step, 1, 32, "_-") {
                    log(
                        self.options.debug,
                        "drop onboarding step: ^[a-z0-9_-]{1,32}$",
                    );
                } else if !matches!(status.as_str(), "ok" | "fail" | "skip") {
                    log(self.options.debug, "drop onboarding status: ok|fail|skip");
                } else if reason
                    .as_ref()
                    .is_some_and(|s| !s.is_empty() && !grammar(s, 1, 64, "_.-"))
                {
                    log(
                        self.options.debug,
                        "drop onboarding reason: ^[a-z0-9_.-]+$ and <= 64 chars",
                    );
                } else {
                    let mut props = Props::from([("status".into(), PropValue::String(status))]);
                    if let Some(reason) = reason.filter(|s| !s.is_empty()) {
                        props.insert("reason".into(), PropValue::String(reason));
                    }
                    self.track(&format!("onboarding:{step}"), props);
                }
            }
            Command::SetProps(props) if self.active => self.set_props(props),
            Command::InstallId => result = serde_json::json!(self.store.state.install_id),
            Command::Reset if self.active => {
                self.cancel_request();
                let key = self.store.state.key.clone();
                let app = self.store.state.app.clone();
                let props = self.store.state.install_props.clone();
                self.store.clear();
                self.store.state.install_props = props;
                self.fresh_identity(key, app);
            }
            Command::Disable => {
                self.cancel_request();
                self.active = false;
                self.init_flush = None;
                self.track_flush = None;
                self.pending = false;
                self.client = None;
                // A pre-init disable is inert and never touches storage.
                if !self.store.state.install_id.is_empty() {
                    self.store.wipe();
                }
            }
            Command::Flush if self.active => self.pending = true,
            #[cfg(any(test, feature = "conformance"))]
            Command::Export => {
                result = self.store.export();
                let receipt_bytes = self.request.as_ref().map_or(0, |request| {
                    request.ids.capacity() * std::mem::size_of::<String>()
                        + request.ids.iter().map(String::capacity).sum::<usize>()
                });
                result["accounted_state_bytes"] = serde_json::json!(
                    self.store.accounted_bytes() + std::mem::size_of::<Self>() + receipt_bytes
                );
            }
            #[cfg(any(test, feature = "conformance"))]
            Command::LegacyVersion => {
                if self.active && !self.store.transition_pending {
                    let previous = std::mem::take(&mut self.store.state.last_app_version);
                    let saved = self.store.checkpoint();
                    if !saved {
                        self.store.state.last_app_version = previous;
                    }
                    result = serde_json::json!(saved);
                } else {
                    result = serde_json::json!(false);
                }
            }
            #[cfg(any(test, feature = "conformance"))]
            Command::Advance(ms) => {
                if let Some(now) = &mut self.options.now {
                    *now += BigInt::from(ms);
                    if let Some(reply) = reply {
                        self.waiters.push(reply);
                    }
                    return true;
                }
                // Unpinned sleep must not block the worker or its network/timers.
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    if let Some(reply) = reply {
                        let _ = reply.send(serde_json::Value::Null);
                    }
                });
                return true;
            }
            #[cfg(any(test, feature = "conformance"))]
            Command::Settle => {
                if let Some(reply) = reply {
                    self.waiters.push(reply);
                }
                return true;
            }
            #[cfg(any(test, feature = "conformance"))]
            Command::Quit => {
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
                return false;
            }
            _ => {}
        }
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
        true
    }
    fn initialize(&mut self, key: String, app: Option<String>, endpoint: Option<String>) {
        if self.active {
            log(self.options.debug, "init ignored: already initialized");
            return;
        }
        if !key
            .strip_prefix("prd_")
            .is_some_and(|s| grammar(s, 10, 10, ""))
        {
            log(
                self.options.debug,
                "drop init: product key must match ^prd_[a-z0-9]{10}$",
            );
            return;
        }
        let app = app.filter(|s| !s.is_empty()).filter(|s| {
            let valid = grammar(s, 1, 32, "-");
            if !valid {
                log(self.options.debug, "drop app slug: ^[a-z0-9-]{1,32}$");
            }
            valid
        });
        // An empty explicit endpoint is absent (JELTO_ENDPOINT applies next);
        // any other value, explicit or from the environment, must be an
        // absolute http(s) URL with no userinfo or the SDK stays inactive.
        let endpoint = endpoint
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| self.options.endpoint.clone());
        if !valid_endpoint(&endpoint) {
            log(
                self.options.debug,
                "drop init: endpoint must be an absolute http(s) URL without userinfo",
            );
            return;
        }
        let metadata = Metadata::detect(
            &self.options.av,
            client_version(self.options.version.clone(), self.options.debug),
        );
        if !supported(&metadata) {
            log(self.options.debug, "drop init: unsupported desktop platform");
            return;
        }
        self.endpoint = endpoint;
        self.metadata = Some(metadata);
        self.store.load();
        self.active = true;
        self.install_enqueued = self.store.contains("install");
        if self.store.state.key != key || self.store.state.app != app {
            self.store.clear();
        }
        if self.store.state.install_id.is_empty() {
            self.fresh_identity(key, app);
        } else {
            self.init_flush = Some(self.now() + 2000);
            self.pending = false; // relaunch flush starts at 2 s, still respecting persisted backoff
            self.observe_version();
            self.heartbeat(&self.now());
            self.store.checkpoint();
        }
    }
    fn fresh_identity(&mut self, key: String, app: Option<String>) {
        let now = self.now();
        self.install_enqueued = false;
        self.store.state.key = key;
        self.store.state.app = app;
        self.store.state.install_id = uuid::Uuid::new_v4().to_string();
        self.store.state.install_due_at =
            (&now + BigInt::from(rand::random_range(0..=INSTALL_DELAY))).to_string();
        self.init_flush = Some(&now + 2000);
        self.track_flush = None;
        self.pending = false;
        self.observe_version();
        self.heartbeat(&now);
        self.store.checkpoint(); // deadline and identity durable before any following call replies
    }
    fn event(&self, name: &str, now: &BigInt, props: Props, heartbeat: bool) -> Event {
        let mut event = Event::new(name, now, props, heartbeat);
        event.metadata = self
            .metadata
            .as_ref()
            .map(|metadata| metadata.snapshot(&self.store.state.app));
        event
    }
    fn observe_version(&mut self) {
        let current = &self.options.av;
        let previous = &self.store.state.last_app_version;
        if !known_app_version(current) || previous == current {
            return;
        }
        let event = if known_app_version(previous) {
            let props = Props::from([
                ("from_version".into(), PropValue::String(previous.clone())),
                ("to_version".into(), PropValue::String(current.clone())),
            ]);
            Some(self.event("app_updated", &self.now(), props, false))
        } else {
            None
        };
        self.store.observe_version(&self.options.av, event);
    }
    fn heartbeat(&mut self, now: &BigInt) {
        let day = now.div_floor(&BigInt::from(DAY)).to_string();
        if self.store.state.last_heartbeat_day != day {
            self.store.state.last_heartbeat_day = day;
            self.store
                .append(self.event("heartbeat", now, Props::new(), true));
            self.store.checkpoint();
        }
    }
    fn track(&mut self, name: &str, props: Props) {
        if !grammar(name, 1, 64, "_:.-") {
            log(
                self.options.debug,
                "drop event: spec/wire-v1.md §3 n is ^[a-z0-9_:.-]{1,64}$",
            );
            return;
        }
        if !valid_props(&props, self.options.debug) {
            return;
        }
        let now = self.now();
        self.store.append(self.event(name, &now, props, false));
        self.track_flush = Some(now + 5000);
    }
    fn set_props(&mut self, props: Props) {
        let mut merged = self.store.state.install_props.clone();
        for (key, value) in props {
            if !grammar(&key, 1, 32, "_") {
                log(
                    self.options.debug,
                    "drop install property key: ^[a-z0-9_]{1,32}$",
                );
                continue;
            }
            match value {
                PropValue::String(value) if grammar(&value, 1, 24, "_.-") => {
                    merged.insert(key, value);
                }
                _ => log(
                    self.options.debug,
                    "drop install property value: ^[a-z0-9_.-]{1,24}$",
                ),
            }
        }
        if merged.len() > 20 {
            log(self.options.debug, "drop setprops: caps them at 20");
            return;
        }
        if merged == self.store.state.install_props {
            return;
        }
        self.store.state.install_props = merged;
        self.store
            .append(self.event("heartbeat", &self.now(), Props::new(), true));
        self.store.checkpoint();
        self.pending = true;
    }
    fn step(&mut self, now: &BigInt) {
        if !self.active {
            return;
        }
        self.heartbeat(now);
        let state = &mut self.store.state;
        if !state.install_claimed
            && whole(&state.install_first_try).is_some_and(|first| now >= &(first + CLAIM_AFTER))
        {
            state.install_claimed = true;
            self.store.checkpoint();
        }
        if !self.store.state.install_claimed
            && !self.install_enqueued
            && whole(&self.store.state.install_due_at).is_some_and(|due| now >= &due)
            && !self.store.contains("install")
        {
            if self.store.state.install_first_try.is_empty() {
                self.store.state.install_first_try = now.to_string();
            }
            self.install_enqueued = true;
            self.store
                .append(self.event("install", now, Props::new(), false));
            self.store.checkpoint();
            self.pending = true;
        }
        if self.request.is_some() {
            return;
        }
        if self.store.transition_pending && !self.store.checkpoint() {
            return;
        }
        if whole(&self.store.state.stop_until).is_some_and(|until| now < &until) {
            return;
        }
        if !self.store.state.stop_until.is_empty() {
            log(
                self.options.debug,
                "kill switch elapsed (spec/wire-v1.md §8)",
            );
            self.store.state.stop_until.clear();
            self.store.checkpoint();
        }
        if whole(&self.store.state.backoff_next_at).is_some_and(|next| now < &next) {
            return;
        }
        if self.init_flush.as_ref().is_some_and(|due| now >= due) {
            self.init_flush = None;
            self.pending = true;
        }
        if self.track_flush.as_ref().is_some_and(|due| now >= due) {
            self.track_flush = None;
            self.pending = true;
        }
        if self.store.state.stop_probe_due || self.pending {
            if self.store.state.retry.is_none() {
                self.store.state.retry = self.batch(now, self.store.state.stop_probe_due);
                if self.store.state.retry.is_none() {
                    self.pending = false;
                    return;
                }
                self.store.checkpoint();
            }
            if let Some(batch) = &self.store.state.retry {
                if self.client.is_none() {
                    self.client = reqwest::Client::builder()
                        .timeout(Duration::from_secs(5))
                        .redirect(reqwest::redirect::Policy::none())
                        .retry(reqwest::retry::never())
                        .pool_max_idle_per_host(1)
                        .build()
                        .ok();
                }
                if self.options.debug {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr().lock(), "{}", batch.body);
                }
                // Queue eviction may retire the retry while HTTP is still in flight.
                // Its final response must still acknowledge the actual sent events.
                self.request = Some(Flight {
                    task: tokio::spawn(post(
                        self.client.clone(),
                        self.endpoint.clone(),
                        self.options.mock.clone(),
                        batch.body.clone(),
                        self.options.debug,
                    )),
                    ids: batch.ids.clone(),
                    install: batch.install,
                    probe: batch.probe,
                });
            }
        }
    }
    fn next_wait(&self, now: &BigInt) -> Duration {
        if !self.active {
            return Duration::from_secs(86400);
        }
        if self.store.transition_pending {
            return Duration::from_secs(1);
        }
        let mut candidates = vec![
            self.init_flush.clone(),
            self.track_flush.clone(),
            whole(&self.store.state.backoff_next_at),
            whole(&self.store.state.stop_until),
            Some((now.div_floor(&BigInt::from(DAY)) + 1) * DAY),
        ];
        if !self.store.state.install_claimed {
            candidates.push(whole(&self.store.state.install_due_at));
            candidates.push(whole(&self.store.state.install_first_try).map(|v| v + CLAIM_AFTER));
        }
        let ms = candidates
            .into_iter()
            .flatten()
            .filter(|v| v > now)
            .map(|v| (v - now).to_u64().unwrap_or(DAY))
            .min()
            .unwrap_or(DAY);
        Duration::from_millis(ms.min(DAY))
    }
    fn batch(&mut self, now: &BigInt, probe: bool) -> Option<Batch> {
        let metadata = self.metadata.as_ref()?;
        let state = &self.store.state;
        let mut batch = Batch {
            probe,
            body: format!(
                "{{\"v\":1,\"p\":{},\"e\":[",
                serde_json::to_string(&state.key).ok()?
            ),
            ..Batch::default()
        };
        let probe_event = self.event("heartbeat", now, Props::new(), true);
        let events: Vec<Event> = if probe {
            vec![probe_event]
        } else {
            self.store
                .queue
                .iter()
                .take(100)
                .filter_map(|s| serde_json::from_str(s).ok())
                .collect()
        };
        // Neither an unrenderable event nor one whose rendered form alone exceeds
        // MAX_BYTES can ever be sent; either would wedge every later event behind
        // it forever if it just broke out of the loop, so it is retired instead.
        let mut unsendable = Vec::new();
        for event in events {
            let Some(rendered) =
                metadata.render(&event, &state.install_id, &state.app, &state.install_props)
            else {
                log(self.options.debug, "drop event: unrenderable for this platform");
                unsendable.push(event.id);
                continue;
            };
            let separator = if batch.ids.is_empty() { "" } else { "," };
            if batch.body.len() + rendered.len() + separator.len() + 2 > MAX_BYTES {
                if batch.ids.is_empty() {
                    log(self.options.debug, "drop event: exceeds MAX_BYTES alone");
                    unsendable.push(event.id);
                    continue;
                }
                break;
            }
            batch.body.push_str(separator);
            batch.body.push_str(&rendered);
            batch.ids.push(event.id);
            batch.install |= event.n == "install";
        }
        if !unsendable.is_empty() {
            self.store.remove(&unsendable);
        }
        if batch.ids.is_empty() {
            return None;
        }
        batch.body.push_str("]}");
        Some(batch)
    }
    fn answered(&mut self, batch: Flight, outcome: Outcome) {
        if !self.active {
            return;
        }
        let now = self.now();
        if outcome.retryable() {
            let (wait, next_step) = backoff(
                self.store.state.backoff_step_ms,
                outcome.retry_after.as_deref(),
                self.options.debug,
            );
            self.store.state.backoff_step_ms = next_step;
            self.store.state.backoff_next_at = (&now + BigInt::from(wait) + 1u32).to_string();
            self.pending = true;
            log(
                self.options.debug,
                &format!("request retry in {wait} ms status={}", outcome.status),
            );
        } else {
            self.store.state.retry = None;
            if !batch.probe {
                self.store.remove(&batch.ids);
            }
            self.store.state.backoff_step_ms = 0;
            self.store.state.backoff_next_at.clear();
            if outcome.status == 202 && batch.install {
                self.store.state.install_claimed = true;
            }
            if batch.probe {
                self.store.state.stop_probe_due = false;
            }
            self.apply_response(&outcome.body, &now, batch.probe);
            if outcome.status != 202 {
                let reason = if outcome.status == 402 {
                    " payment_required"
                } else {
                    ""
                };
                log(
                    self.options.debug,
                    &format!(
                        "batch dropped: status={}{reason}; final, not retried",
                        outcome.status
                    ),
                );
            }
            self.pending = !self.store.queue.is_empty();
        }
        self.store.checkpoint();
    }
    fn apply_response(&mut self, body: &serde_json::Value, now: &BigInt, probe: bool) {
        if let Some(rejected) = body.get("rejected").and_then(serde_json::Value::as_array) {
            // Do not echo arbitrary remote free text into logs.
            for item in rejected {
                if let Some(reason) = item
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| grammar(s, 1, 64, "_"))
                {
                    log(self.options.debug, &format!("event rejected: {reason}"));
                }
            }
        }
        let Some(stop) = body.get("stop") else {
            return;
        };
        match stop.get("scope").and_then(serde_json::Value::as_str) {
            Some("web") => {
                log(self.options.debug, "ignoring a stop scoped to web");
                return;
            }
            Some("app") => {}
            _ => return,
        }
        let Some(until) = stop
            .get("until")
            .filter(|v| v.is_number())
            .and_then(|v| whole(&v.to_string()))
        else {
            return;
        };
        let until = until * 1000u32;
        if &until <= now {
            log(self.options.debug, "stop until is already past");
            // This request already discharged the one-heartbeat obligation for an
            // elapsed pause. Re-arming here would create an unbounded probe loop.
            if probe {
                return;
            }
        }
        self.store.state.stop_until = until.to_string();
        self.store.state.stop_probe_due = true;
        log(
            self.options.debug,
            "kill switch: no request until stop.until",
        );
    }
}

#[cfg(test)]
pub(crate) fn test_engine(options: Options) -> Jelto {
    Jelto::start(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepted_in_flight_batch_is_handled_after_its_oldest_event_is_evicted() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::new(Options {
            dir: Some(dir.path().into()),
            av: "release A".into(),
            now: Some(BigInt::from(0)),
            endpoint: "http://127.0.0.1:1/v1/e".into(),
            debug: false,
            version: None,
            mock: None,
        });
        worker.initialize("prd_conform001".into(), None, None);
        worker.store.queue.clear();
        worker.store.bytes = 0;
        worker.options.av = "release B".into();
        worker.metadata = Some(Metadata::detect("release B", None));
        worker.observe_version();
        let update_id = serde_json::from_str::<Event>(&worker.store.queue[0])
            .unwrap()
            .id;
        worker
            .store
            .append(worker.event("install", &BigInt::from(0), Props::new(), false));
        worker.install_enqueued = true;
        for _ in 2..QUEUE_EVENTS {
            worker.track("before_response", Props::new());
        }
        worker.pending = true;
        worker.step(&BigInt::from(3000));
        let accepted_ids = worker.store.state.retry.as_ref().unwrap().ids.clone();
        assert_eq!(accepted_ids.len(), 100);
        assert_eq!(accepted_ids[0], update_id);
        worker.track("after_request", Props::new());
        assert!(worker.store.state.retry.is_none());
        assert_eq!(worker.store.queue.len(), QUEUE_EVENTS);

        let request = worker.request.take().unwrap();
        request.task.abort();
        worker.answered(
            request,
            Outcome {
                status: 202,
                retry_after: None,
                body: serde_json::json!({"stop": {"scope": "app", "until": 60}}),
            },
        );
        assert!(
            worker.store.state.install_claimed,
            "accepted install was ignored"
        );
        assert_eq!(worker.store.state.stop_until, "60000");
        assert!(worker.store.state.stop_probe_due);
        assert!(worker.store.queue.iter().all(|line| {
            let event: Event = serde_json::from_str(line).unwrap();
            !accepted_ids.contains(&event.id)
        }));
        assert!(worker.store.contains("after_request"));
    }

    #[tokio::test]
    async fn failed_transition_persistence_blocks_dispatch_until_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::new(Options {
            dir: Some(dir.path().into()),
            av: "release A".into(),
            now: Some(BigInt::from(0)),
            endpoint: "http://127.0.0.1:1/v1/e".into(),
            debug: false,
            version: None,
            mock: None,
        });
        worker.initialize("prd_conform001".into(), None, None);
        worker.store.state.install_claimed = true;
        worker.store.queue.clear();
        worker.store.bytes = 0;
        assert!(worker.store.checkpoint());
        worker.options.av = "release B".into();
        worker.metadata = Some(Metadata::detect("release B", None));
        worker.store.interrupt_checkpoint(Some(false));
        worker.observe_version();
        let event_id = serde_json::from_str::<Event>(&worker.store.queue[0])
            .unwrap()
            .id;
        worker.pending = true;
        worker.step(&BigInt::from(3000));
        assert!(worker.store.transition_pending);
        assert!(worker.request.is_none());
        assert!(worker.store.state.retry.is_none());
        let mut recovered = Store::new(Some(dir.path().into()), false);
        recovered.load();
        assert_eq!(recovered.state.last_app_version, "release A");
        assert!(recovered.queue.is_empty());

        worker.store.interrupt_checkpoint(None);
        worker.step(&BigInt::from(3000));
        assert!(!worker.store.transition_pending);
        assert!(worker.request.is_some());
        assert_eq!(
            worker.store.state.retry.as_ref().unwrap().ids,
            vec![event_id]
        );
        worker.cancel_request();
    }

    fn conformance_worker(dir: &std::path::Path) -> Worker {
        let mut worker = Worker::new(Options {
            dir: Some(dir.into()),
            av: "release A".into(),
            now: Some(BigInt::from(0)),
            endpoint: "http://127.0.0.1:1/v1/e".into(),
            debug: false,
            version: None,
            mock: None,
        });
        worker.initialize("prd_conform001".into(), None, None);
        worker.store.queue.clear();
        worker.store.bytes = 0;
        worker
    }

    /// A queue entry can only ever reach this size if it slipped past
    /// `valid_props` (e.g. a persisted record from before this cap existed);
    /// `batch()` must retire it rather than wedge on it forever.
    #[test]
    fn one_oversized_event_cannot_block_every_later_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = conformance_worker(dir.path());
        let digits = "9".repeat(70_000);
        let huge_props: Props = serde_json::from_str(&format!(r#"{{"n":{digits}}}"#)).unwrap();
        let huge = worker.event("huge", &BigInt::from(0), huge_props, false);
        let huge_id = huge.id.clone();
        worker.store.append(huge);
        worker.track("after", Props::new());
        let after_id = serde_json::from_str::<Event>(&worker.store.queue[1])
            .unwrap()
            .id;

        let batch = worker.batch(&BigInt::from(0), false).unwrap();
        assert!(batch.body.len() <= MAX_BYTES);
        assert_eq!(batch.ids, vec![after_id]);
        assert!(worker
            .store
            .queue
            .iter()
            .all(|line| serde_json::from_str::<Event>(line).unwrap().id != huge_id));
    }

    /// An event whose attached (or, for legacy records, fallback) metadata is
    /// invalid can never be rendered; `batch()` must retire it too instead of
    /// blocking every later event behind it.
    #[test]
    fn unrenderable_event_is_dropped_without_blocking_later_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = conformance_worker(dir.path());
        let bogus = Metadata {
            av: "release A".into(),
            os: "offbrand",
            osv: "1".into(),
            arch: "arm64",
            version: None,
        };
        let mut broken = worker.event("broken", &BigInt::from(0), Props::new(), false);
        broken.metadata = Some(bogus.snapshot(&None));
        let broken_id = broken.id.clone();
        worker.store.append(broken);
        worker.track("after", Props::new());
        let after_id = serde_json::from_str::<Event>(&worker.store.queue[1])
            .unwrap()
            .id;

        let batch = worker.batch(&BigInt::from(0), false).unwrap();
        assert_eq!(batch.ids, vec![after_id]);
        assert!(worker
            .store
            .queue
            .iter()
            .all(|line| serde_json::from_str::<Event>(line).unwrap().id != broken_id));
    }

    #[test]
    fn supported_rejects_an_empty_os_or_architecture() {
        assert!(supported(&Metadata {
            av: "1.0.0".into(),
            os: "linux",
            osv: String::new(),
            arch: "x64",
            version: None,
        }));
        assert!(!supported(&Metadata {
            av: "1.0.0".into(),
            os: "",
            osv: String::new(),
            arch: "x64",
            version: None,
        }));
        assert!(!supported(&Metadata {
            av: "1.0.0".into(),
            os: "linux",
            osv: String::new(),
            arch: "",
            version: None,
        }));
    }

    #[test]
    fn valid_endpoint_rejects_userinfo_non_http_and_unparseable_urls() {
        assert!(valid_endpoint("https://in.jelto.io/v1/e"));
        assert!(valid_endpoint("http://127.0.0.1:8080/v1/e"));
        assert!(!valid_endpoint("file:///dev/null"));
        assert!(!valid_endpoint("https://u:p@example.com/v1/e"));
        assert!(!valid_endpoint("not a url"));
        assert!(!valid_endpoint(""));
    }
}
