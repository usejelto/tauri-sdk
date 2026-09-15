use crate::{
    engine::{test_engine, Options},
    wire::*,
    Jelto, Props,
};
use num_bigint::BigInt;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const KEY: &str = "prd_conform001";
fn engine(dir: &Path, endpoint: &str, pin: &str) -> Jelto {
    versioned_engine(dir, endpoint, pin, "2.3.4")
}
fn versioned_engine(dir: &Path, endpoint: &str, pin: &str, version: &str) -> Jelto {
    test_engine(Options {
        dir: Some(dir.into()),
        av: version.into(),
        now: whole(pin),
        endpoint: endpoint.into(),
        debug: false,
        version: None,
        mock: None,
    })
}

fn queued_updates(state: &Value) -> Vec<String> {
    state["queue"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["n"] == "app_updated")
        .map(|e| e["id"].as_str().unwrap().into())
        .collect()
}

#[tokio::test]
async fn version_baselines_legacy_unknown_versions_and_identity_controls() {
    let dir = tempfile::tempdir().unwrap();
    let offline = "http://127.0.0.1:1/v1/e";
    let mut previous_id = String::new();
    for version in ["", " \t\n", &"界".repeat(33), " Release A+build "] {
        let sdk = versioned_engine(dir.path(), offline, "0", version);
        assert!(!sdk.legacy_version().await);
        sdk.init(KEY, None, None).await;
        let state = sdk.export_state().await;
        assert!(queued_updates(&state).is_empty());
        if !previous_id.is_empty() {
            assert_eq!(state["install_id"], previous_id);
        }
        previous_id = sdk.install_id().await;
        assert_eq!(
            state["last_app_version"],
            if known_app_version(version) {
                version
            } else {
                ""
            }
        );
        drop(sdk);
    }
    for version in ["", " \t", &"x".repeat(33)] {
        let sdk = versioned_engine(dir.path(), offline, "0", version);
        sdk.init(KEY, None, None).await;
        assert_eq!(
            sdk.export_state().await["last_app_version"],
            " Release A+build "
        );
        assert!(queued_updates(&sdk.export_state().await).is_empty());
        drop(sdk);
    }
    let sdk = versioned_engine(dir.path(), offline, "0", " Release A+build ");
    sdk.init(KEY, None, None).await;
    let before = sdk.export_state().await;
    assert!(sdk.legacy_version().await);
    let mut after = sdk.export_state().await;
    after["last_app_version"] = before["last_app_version"].clone();
    // Accounting changes with the removed string; semantic state is preserved.
    after["accounted_state_bytes"] = before["accounted_state_bytes"].clone();
    assert_eq!(after, before);
    drop(sdk);
    let sdk = versioned_engine(dir.path(), offline, "0", "release B");
    sdk.init(KEY, None, None).await;
    assert!(queued_updates(&sdk.export_state().await).is_empty());
    assert_eq!(sdk.install_id().await, previous_id);
    assert_eq!(sdk.export_state().await["last_app_version"], "release B");
    sdk.reset().await;
    assert_ne!(sdk.install_id().await, previous_id);
    assert_eq!(sdk.export_state().await["last_app_version"], "release B");
    assert!(queued_updates(&sdk.export_state().await).is_empty());
    sdk.disable().await;
    assert!(!dir.path().exists());
    sdk.init(KEY, None, None).await;
    assert_eq!(sdk.export_state().await["last_app_version"], "release B");
    assert!(queued_updates(&sdk.export_state().await).is_empty());
    sdk.disable().await;
}

#[tokio::test]
async fn offline_same_day_updates_downgrades_and_retries_preserve_each_transition() {
    let first_server = Server::new(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let sdk = versioned_engine(dir.path(), &first_server.endpoint, "0", " release A+one ");
    sdk.init(KEY, Some("desktop"), None).await;
    sdk.advance(3000).await;
    let id = sdk.install_id().await;
    let installed = sdk.export_state().await;
    assert_eq!(installed["install_claimed"], true);
    sdk.stop().await;
    let mut ids = Vec::new();
    for version in ["release B", " release A+one ", "release B"] {
        let sdk = versioned_engine(dir.path(), "http://127.0.0.1:1/v1/e", "21600002", version);
        sdk.init(KEY, Some("desktop"), None).await;
        let state = sdk.export_state().await;
        assert_eq!(state["install_id"], id);
        assert_eq!(state["install_claimed"], true);
        assert_eq!(state["install_due_at"], installed["install_due_at"]);
        assert_eq!(state["install_first_try"], installed["install_first_try"]);
        assert_eq!(state["last_heartbeat_day"], installed["last_heartbeat_day"]);
        let updates = queued_updates(&state);
        assert_eq!(updates.len(), ids.len() + 1);
        assert_eq!(&updates[..ids.len()], &ids);
        ids = updates;
        drop(sdk); // no flush; checkpointed transitions survive an abrupt launch end
    }
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    let server = Server::new(vec![(429, "Retry-After: 2\r\n", "{}", 0)]).await;
    let sdk = versioned_engine(dir.path(), &server.endpoint, "21600002", "release B");
    sdk.init(KEY, Some("desktop"), None).await;
    sdk.advance(3000).await;
    assert_eq!(queued_updates(&sdk.export_state().await), ids);
    drop(sdk); // response received, retry persisted, process exits without a flush
    let sdk = versioned_engine(dir.path(), &server.endpoint, "21603002", "release B");
    sdk.init(KEY, Some("desktop"), None).await;
    sdk.advance(3000).await;
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    let body: Value = serde_json::from_str(&requests[1]).unwrap();
    let events = body["e"].as_array().unwrap();
    assert_eq!(events.len(), 3); // same-day launch adds no heartbeat or install
    for (event, (from, to)) in events.iter().zip([
        (" release A+one ", "release B"),
        ("release B", " release A+one "),
        (" release A+one ", "release B"),
    ]) {
        assert_eq!(event["n"], "app_updated");
        assert_eq!(event["iid"], id);
        assert_eq!(event["av"], to);
        assert_eq!(event["props"], json!({"from_version":from,"to_version":to}));
        assert_eq!(event["t"], 21600002);
    }
    assert!(queued_updates(&sdk.export_state().await).is_empty());
    sdk.disable().await;
}

#[test]
fn transition_metadata_and_legacy_queue_records_render_consistently() {
    let mut old = Event::new("app_updated", &BigInt::from(5), Props::new(), false);
    let observed = Metadata {
        av: "Build A".into(),
        os: "macos",
        osv: "15.0".into(),
        arch: "arm64",
        version: Some("tauri/0.1.0".into()),
    };
    old.metadata = Some(observed.snapshot(&Some("desktop".into())));
    let current = Metadata {
        av: "Build B".into(),
        os: "windows",
        osv: "20.0".into(),
        arch: "x64",
        version: Some("tauri/0.2.0".into()),
    };
    let rendered: Value = serde_json::from_str(
        &current
            .render(&old, "id", &Some("other".into()), &BTreeMap::new())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rendered["av"], "Build A");
    assert_eq!(rendered["os"], "macos");
    assert_eq!(rendered["osv"], "15.0");
    assert_eq!(rendered["arch"], "arm64");
    assert_eq!(rendered["a"], "desktop");
    assert_eq!(rendered["v"], "tauri/0.1.0");
    let legacy: Event =
        serde_json::from_value(json!({"id":uuid::Uuid::new_v4().to_string(),"n":"old","t":"0"}))
            .unwrap();
    assert!(legacy.valid());
    assert!(current
        .render(&legacy, "id", &None, &BTreeMap::new())
        .is_some());
}
fn props(value: Value) -> Props {
    serde_json::from_value(value).unwrap()
}

#[test]
fn valid_props_rejects_a_number_literal_over_200_chars_but_accepts_200() {
    let over: Props = serde_json::from_str(&format!(r#"{{"n":{}}}"#, "9".repeat(201))).unwrap();
    assert!(!valid_props(&over, false));
    let at_cap: Props = serde_json::from_str(&format!(r#"{{"n":{}}}"#, "9".repeat(200))).unwrap();
    assert!(valid_props(&at_cap, false));
}

struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn new(script: Vec<(u16, &'static str, &'static str, u64)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/e", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            let mut index = 0;
            while let Ok((mut socket, _)) = listener.accept().await {
                let captured = captured.clone();
                let (status, header, body, delay) =
                    script.get(index).copied().unwrap_or((202, "", "{}", 0));
                index += 1;
                tokio::spawn(async move {
                    let mut data = Vec::new();
                    let mut buf = [0; 8192];
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        data.extend_from_slice(&buf[..n]);
                        if let Some(end) = data.windows(4).position(|s| s == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                            let length = headers
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length: "))
                                .unwrap()
                                .parse::<usize>()
                                .unwrap();
                            if data.len() >= end + 4 + length {
                                captured.lock().unwrap().push(
                                    String::from_utf8(data[end + 4..end + 4 + length].to_vec())
                                        .unwrap(),
                                );
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    let reply = format!("HTTP/1.1 {status} Answer\r\nContent-Length: {}\r\nConnection: close\r\n{header}\r\n{body}", body.len());
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }
    fn bodies(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    async fn wait_requests(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.requests.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn inert_registration_restart_properties_reset_and_disable() {
    let dir = tempfile::tempdir().unwrap();
    let sdk = engine(dir.path(), "http://127.0.0.1:1", "0");
    sdk.track("ignored", None).await;
    sdk.disable().await;
    assert_eq!(sdk.install_id().await, "");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    sdk.init(KEY, Some("desktop"), None).await;
    let id = sdk.install_id().await;
    let initial = sdk.export_state().await;
    sdk.set_props(props(json!({"license":"paid","invalid":"X@Y", "number":1})))
        .await;
    let due = initial["install_due_at"].clone();
    assert_eq!(due, "0");
    assert_eq!(
        initial["queue"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["n"] == "install")
            .count(),
        1
    );
    drop(sdk); // no orderly termination callback
    let second = engine(dir.path(), "http://127.0.0.1:1", "1000");
    second.init(KEY, Some("desktop"), None).await;
    assert_eq!(second.install_id().await, id);
    assert_eq!(second.export_state().await["install_due_at"], due);
    assert_eq!(
        second.export_state().await["install_props"],
        json!({"license":"paid"})
    );
    second.track("old", None).await;
    second.reset().await;
    assert_ne!(second.install_id().await, id);
    let state = second.export_state().await;
    assert_eq!(state["install_due_at"], "1000");
    let names = state["queue"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["n"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["heartbeat", "install"]);
    assert_eq!(state["install_props"], json!({"license":"paid"}));
    second.disable().await;
    assert!(!dir.path().exists());
    second.init(KEY, None, None).await;
    assert!(!second.install_id().await.is_empty());
    assert_eq!(second.export_state().await["install_props"], json!({}));
    second.disable().await;
}

#[tokio::test]
async fn queue_caps_oldest_first_and_corrupt_or_unavailable_storage() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.jsonl"), "{broken checkpoint\n").unwrap();
    let sdk = engine(dir.path(), "http://127.0.0.1:1", "0");
    sdk.init(KEY, None, None).await;
    for i in 0..1500 {
        sdk.track(&format!("x{i}"), None).await;
    }
    let state = sdk.export_state().await;
    assert_eq!(state["queue"]["events"].as_array().unwrap().len(), 1000);
    assert_eq!(state["queue"]["events"][0]["n"], "x500");
    let large = props(Value::Object(
        (0..20)
            .map(|i| (format!("k{i}"), json!("界".repeat(200))))
            .collect(),
    ));
    for _ in 0..120 {
        sdk.track("large", Some(large.clone())).await;
    }
    let state = sdk.export_state().await;
    assert!(state["queue"]["bytes"].as_u64().unwrap() <= QUEUE_BYTES as u64);
    assert!(state["accounted_state_bytes"].as_u64().unwrap() <= 2 * QUEUE_BYTES as u64);
    sdk.disable().await;
    let file = tempfile::NamedTempFile::new().unwrap();
    let sdk = engine(file.path(), "http://127.0.0.1:1", "0");
    sdk.init(KEY, None, None).await;
    assert!(!sdk.install_id().await.is_empty());
    sdk.track("memory", None).await;
    assert_eq!(
        sdk.export_state().await["queue"]["events"][2]["n"],
        "memory"
    );
    sdk.disable().await;
}

#[tokio::test]
async fn exact_timestamps_retry_ids_backoff_and_stop_probe() {
    let server = Server::new(vec![
        (429, "Retry-After: 2\r\n", "{}", 0),
        (
            202,
            "",
            r#"{"stop":{"scope":"app","until":9223372036854780.0}}"#,
            0,
        ),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let pin = "9223372036854775809";
    let sdk = engine(dir.path(), &server.endpoint, pin);
    sdk.init(KEY, Some("desktop"), None).await;
    sdk.track(
        "export",
        Some(props(json!({"format":"pdf","count":1,"ok":true}))),
    )
    .await;
    sdk.advance(2000).await;
    let first = sdk.export_state().await;
    assert_eq!(first["backoff_step_ms"], 2000);
    sdk.advance(2001).await;
    let bodies = server.requests.lock().unwrap().clone();
    assert_eq!(bodies[0], bodies[1]);
    assert!(bodies[0].contains(pin));
    sdk.track("paused", None).await;
    sdk.advance(10_000).await;
    let bodies = server.bodies();
    assert_eq!(bodies[2]["e"].as_array().unwrap().len(), 1);
    assert_eq!(bodies[2]["e"][0]["n"], "heartbeat");
    assert_eq!(bodies[3]["e"][0]["n"], "paused");
    sdk.disable().await;
}

#[tokio::test]
async fn malformed_final_responses_request_limits_and_concurrency() {
    let server = Server::new(vec![
        (202, "", "not-json", 0),
        (500, "", "oops", 0),
        (402, "", r#"{"error":"payment_required"}"#, 0),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let sdk = engine(dir.path(), &server.endpoint, "1788134400000");
    sdk.init(KEY, None, None).await;
    let large = props(Value::Object(
        (0..20)
            .map(|i| (format!("k{i}"), json!("x".repeat(200))))
            .collect(),
    ));
    let mut jobs = Vec::new();
    for _ in 0..8 {
        let sdk = sdk.clone();
        let large = large.clone();
        jobs.push(tokio::spawn(async move {
            for _ in 0..32 {
                sdk.track("x", Some(large.clone())).await;
            }
        }));
    }
    for job in jobs {
        job.await.unwrap();
    }
    sdk.advance(6000).await;
    for raw in server.requests.lock().unwrap().iter() {
        assert!(raw.len() <= MAX_BYTES);
        assert!(
            serde_json::from_str::<Value>(raw).unwrap()["e"]
                .as_array()
                .unwrap()
                .len()
                <= 100
        );
    }
    assert_eq!(sdk.export_state().await["queue"]["bytes"], 0);
    assert_eq!(sdk.export_state().await["backoff_step_ms"], 0);
    sdk.disable().await;
}

#[tokio::test]
async fn disable_and_reset_cancel_inflight_results() {
    for disable in [true, false] {
        let server = Server::new(vec![(
            202,
            "",
            r#"{"stop":{"scope":"app","until":9999999999}}"#,
            250,
        )])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let sdk = engine(dir.path(), &server.endpoint, "0");
        sdk.init(KEY, None, None).await;
        let old_id = sdk.install_id().await;
        let driver = sdk.clone();
        let pending = tokio::spawn(async move {
            driver.advance(3000).await;
        });
        server.wait_requests(1).await;
        if disable {
            sdk.disable().await;
        } else {
            sdk.reset().await;
        }
        pending.await.unwrap();
        tokio::time::sleep(Duration::from_millis(350)).await;
        let state = sdk.export_state().await;
        assert_eq!(state["stop_until"], "");
        assert_eq!(state["install_claimed"], false);
        assert_ne!(sdk.install_id().await, old_id);
        if disable {
            assert!(!dir.path().exists());
            assert_eq!(sdk.install_id().await, "");
        }
        sdk.disable().await;
    }
}

#[tokio::test]
async fn daily_utc_rollover_and_install_deadline_survive_restart() {
    let server = Server::new(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let sdk = engine(dir.path(), &server.endpoint, "-1");
    sdk.init(KEY, None, None).await;
    assert_eq!(sdk.export_state().await["last_heartbeat_day"], "-1");
    assert_eq!(sdk.export_state().await["install_due_at"], "-1");
    sdk.advance(1).await;
    assert_eq!(sdk.export_state().await["last_heartbeat_day"], "0");
    sdk.advance(3000).await;
    assert_eq!(sdk.export_state().await["install_claimed"], true);
    let events = server
        .bodies()
        .into_iter()
        .flat_map(|body| body["e"].as_array().unwrap().clone())
        .collect::<Vec<_>>();
    assert_eq!(events.iter().filter(|e| e["n"] == "install").count(), 1);
    sdk.stop().await;
    let second = engine(dir.path(), &server.endpoint, "21600002");
    second.init(KEY, None, None).await;
    second.advance(3000).await;
    assert_eq!(second.export_state().await["install_due_at"], "-1");
    assert_eq!(second.export_state().await["install_claimed"], true);
    assert_eq!(second.export_state().await["queue"]["bytes"], 0);
    second.disable().await;
}

#[tokio::test]
async fn invalid_endpoints_leave_the_sdk_inactive_with_no_request_ever_sent() {
    let server = Server::new(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    // Explicit overrides: an absolute http(s) URL with no userinfo is required;
    // anything else (wrong scheme, embedded credentials, unparseable) drops init.
    for invalid in ["file:///dev/null", "https://u:p@example.com/v1/e", "not a url"] {
        let sdk = engine(dir.path(), &server.endpoint, "0");
        sdk.init(KEY, None, Some(invalid)).await;
        assert_eq!(sdk.install_id().await, "");
        sdk.disable().await;
    }
    // A non-empty invalid JELTO_ENDPOINT (simulated via Options.endpoint, the
    // field Options::environment fills from the env var) is validated the same
    // way at initialize() time, with the same outcome.
    let sdk = test_engine(Options {
        dir: Some(dir.path().into()),
        av: "2.3.4".into(),
        now: whole("0"),
        endpoint: "not a url".into(),
        debug: false,
        version: None,
        mock: None,
    });
    sdk.init(KEY, None, None).await;
    assert_eq!(sdk.install_id().await, "");
    sdk.disable().await;
    assert!(
        server.requests.lock().unwrap().is_empty(),
        "an invalid endpoint must never be reachable, explicit or from the environment"
    );
    // An empty override is absent: the next source (here, the options endpoint
    // standing in for JELTO_ENDPOINT) applies and the SDK activates normally.
    let sdk = engine(dir.path(), &server.endpoint, "0");
    sdk.init(KEY, None, Some("")).await;
    assert!(!sdk.install_id().await.is_empty());
    sdk.advance(2001).await;
    server.wait_requests(1).await;
    sdk.disable().await;
}

#[tokio::test]
async fn mock_header_containing_a_newline_is_omitted_but_delivery_still_succeeds() {
    let server = Server::new(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let sdk = test_engine(Options {
        dir: Some(dir.path().into()),
        av: "2.3.4".into(),
        now: whole("0"),
        endpoint: server.endpoint.clone(),
        debug: false,
        version: None,
        mock: Some("bad\nvalue".into()),
    });
    sdk.init(KEY, None, None).await;
    sdk.advance(2001).await;
    server.wait_requests(1).await;
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    assert_eq!(sdk.export_state().await["backoff_step_ms"], 0);
    sdk.disable().await;
}

#[test]
fn exact_whole_number_response_grammar_and_retry_floors() {
    assert_eq!(
        whole("9.223372036854775809e18"),
        whole("9223372036854775809")
    );
    assert_eq!(whole("-1.000"), Some(BigInt::from(-1)));
    for raw in ["1.2", "1e-1", "NaN", "", "1e9999999"] {
        assert!(whole(raw).is_none());
    }
    for _ in 0..100 {
        assert_eq!(backoff(0, Some("9999"), false), (3_600_000, 2000));
        assert!(backoff(0, Some("soon"), false).0 >= 800);
        assert!(backoff(512000, Some("1"), false).0 >= 409600);
    }
}

#[tokio::test]
async fn expired_stop_on_a_probe_does_not_loop() {
    let stop = r#"{"stop":{"scope":"app","until":0}}"#;
    let server = Server::new(vec![(202, "", stop, 0), (202, "", stop, 0)]).await;
    let dir = tempfile::tempdir().unwrap();
    let sdk = engine(dir.path(), &server.endpoint, "0");
    sdk.init(KEY, None, None).await;
    tokio::time::timeout(Duration::from_secs(2), sdk.advance(3000))
        .await
        .unwrap();
    assert_eq!(server.bodies().len(), 2);
    assert_eq!(sdk.export_state().await["stop_probe_due"], false);
    sdk.track("after", None).await;
    sdk.advance(5000).await;
    assert_eq!(server.bodies()[2]["e"][0]["n"], "after");
    sdk.disable().await;
}

#[tokio::test]
async fn c11_allocation_and_cpu_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let sdk = engine(dir.path(), "http://127.0.0.1:1", "0");
    sdk.init(KEY, None, None).await;
    sdk.install_id().await;
    let mut times = Vec::new();
    for _ in 0..10_000 {
        let start = Instant::now();
        sdk.track("x", None).await;
        times.push(start.elapsed().as_micros());
    }
    times.sort();
    let bytes = sdk.export_state().await["accounted_state_bytes"]
        .as_u64()
        .unwrap();
    println!(
        "C11: accounted_state_bytes={bytes}, track_p99_us={}",
        times[9900]
    );
    assert!(bytes <= 2 * QUEUE_BYTES as u64);
    assert!(
        times[9900] < 1000,
        "per-track p99 exceeded 1 ms: {} us",
        times[9900]
    );
    sdk.disable().await;
}

#[cfg(feature = "tauri")]
mod permissions {
    use super::*;
    use tauri::{
        ipc::{CallbackFn, InvokeBody},
        test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY},
        webview::InvokeRequest,
    };
    use tauri_utils::acl::{
        capability::Capability,
        manifest::{Manifest, PermissionFile},
        resolved::Resolved,
    };

    #[tokio::test]
    async fn missing_plugin_is_inert_for_rust_callers() {
        use crate::JeltoExt;
        let app = tauri::test::mock_app();
        app.jelto().init(KEY, None, None).await;
        app.jelto().track("x", None).await;
        app.jelto().set_props(Props::new()).await;
        app.jelto().onboarding("tour", "ok", None).await;
        app.jelto().reset().await;
        app.jelto().disable().await;
        assert_eq!(app.jelto().install_id().await, "");
    }

    fn check(
        grants: &[&str],
        window: &str,
        origin: &str,
        command: &str,
        args: Value,
    ) -> Result<tauri::ipc::InvokeResponseBody, Value> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files =
            vec![
                toml::from_str::<PermissionFile>(include_str!("../permissions/default.toml"))
                    .unwrap(),
            ];
        for entry in std::fs::read_dir(root.join("permissions/autogenerated/commands")).unwrap() {
            files.push(
                toml::from_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap()).unwrap(),
            );
        }
        let acl = BTreeMap::from([("jelto".into(), Manifest::new(files, None))]);
        let cap: Capability = serde_json::from_value(
            json!({"identifier":"main","windows":["main"],"permissions":grants}),
        )
        .unwrap();
        let resolved = Resolved::resolve(
            &acl,
            BTreeMap::from([("main".into(), cap)]),
            tauri_utils::platform::Target::current(),
        )
        .unwrap();
        let mut context = mock_context(noop_assets());
        *context.runtime_authority_mut() = tauri::runtime_authority!(acl, resolved);
        let app = mock_builder().plugin(crate::init()).build(context).unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, window, Default::default())
            .build()
            .unwrap();
        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: format!("plugin:jelto|{command}"),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: origin.parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
    }
    #[test]
    fn real_tauri_command_acl_default_individual_denial_and_remote_isolation() {
        let local = if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        let commands = [
            (
                "init",
                json!({"key":"invalid","endpoint":"https://example.com/v1/e"}),
            ),
            ("track", json!({"name":"x"})),
            ("onboarding", json!({"step":"tour","status":"ok"})),
            ("set_props", json!({"props":{}})),
            ("install_id", json!({})),
            ("reset", json!({})),
            ("disable", json!({})),
        ];
        for (command, args) in commands {
            assert!(
                check(&["jelto:default"], "main", local, command, args.clone()).is_ok(),
                "default: {command}"
            );
            assert!(
                check(&[], "main", local, command, args.clone()).is_err(),
                "missing permission: {command}"
            );
            assert!(
                check(&["jelto:default"], "other", local, command, args.clone()).is_err(),
                "other window: {command}"
            );
            assert!(
                check(
                    &["jelto:default"],
                    "main",
                    "https://example.com",
                    command,
                    args.clone()
                )
                .is_err(),
                "remote: {command}"
            );
            let allow = format!("jelto:allow-{}", command.replace('_', "-"));
            let deny = format!("jelto:deny-{}", command.replace('_', "-"));
            assert!(
                check(&[&allow], "main", local, command, args.clone()).is_ok(),
                "individual: {command}"
            );
            assert!(
                check(&["jelto:default", &deny], "main", local, command, args).is_err(),
                "deny: {command}"
            );
        }
    }
}
