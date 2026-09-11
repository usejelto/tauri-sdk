use crate::wire::{log, whole, Batch, Event, QUEUE_BYTES, QUEUE_EVENTS};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct State {
    pub key: String,
    pub app: Option<String>,
    pub install_id: String,
    pub last_heartbeat_day: String,
    pub last_app_version: String,
    pub install_claimed: bool,
    pub install_due_at: String,
    pub install_first_try: String,
    pub install_props: BTreeMap<String, String>,
    pub backoff_step_ms: u64,
    pub backoff_next_at: String,
    pub stop_until: String,
    pub stop_probe_due: bool,
    pub retry: Option<Batch>,
}
impl State {
    fn valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.install_id)
            .is_ok_and(|id| id.get_version_num() == 4 && !id.is_nil())
            && [
                &self.install_due_at,
                &self.install_first_try,
                &self.backoff_next_at,
                &self.stop_until,
                &self.last_heartbeat_day,
            ]
            .iter()
            .all(|s| s.is_empty() || whole(s).is_some())
            && self.install_props.len() <= 20
            && self.install_props.iter().all(|(k, v)| {
                crate::wire::grammar(k, 1, 32, "_") && crate::wire::grammar(v, 1, 24, "_.-")
            })
            && self.retry.as_ref().is_none_or(|batch| {
                batch.body.len() <= crate::wire::MAX_BYTES
                    && batch.ids.len() <= 100
                    && serde_json::from_str::<serde_json::Value>(&batch.body).is_ok()
            })
    }
}

/// One checkpoint followed by append-only event lines. Replay enforces the live queue
/// cap. Checkpoints replace the complete file atomically; appends need no full rewrite.
pub(crate) struct Store {
    pub state: State,
    pub queue: VecDeque<String>,
    pub bytes: usize,
    dir: Option<PathBuf>,
    file: Option<File>,
    journal_bytes: usize,
    debug: bool,
    // A failed combined transition checkpoint remains in memory, but cannot be sent.
    pub transition_pending: bool,
    #[cfg(test)]
    fail_checkpoint: Option<bool>,
}
impl Store {
    pub fn new(dir: Option<PathBuf>, debug: bool) -> Self {
        Self {
            state: State::default(),
            queue: VecDeque::new(),
            bytes: 0,
            dir,
            file: None,
            journal_bytes: 0,
            debug,
            transition_pending: false,
            #[cfg(test)]
            fail_checkpoint: None,
        }
    }
    pub fn load(&mut self) {
        let Some(dir) = &self.dir else {
            return;
        };
        let Ok(file) = File::open(dir.join("state.jsonl")) else {
            return;
        };
        // Bound corrupt input and do not replay events under a replacement identity.
        let mut reader = BufReader::new(file.take((4 * QUEUE_BYTES) as u64));
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let Ok(state) = serde_json::from_str::<State>(&line) else {
            log(self.debug, "storage checkpoint corrupt; starting fresh");
            return;
        };
        if !state.valid() {
            log(self.debug, "storage checkpoint invalid; starting fresh");
            return;
        }
        self.state = state;
        loop {
            line.clear();
            if !matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
                break;
            }
            if !line.ends_with('\n') {
                break;
            } // an interrupted append costs only its last event
            if line.len() > crate::wire::MAX_BYTES {
                continue;
            }
            if serde_json::from_str::<Event>(&line).is_ok_and(|e| e.valid()) {
                self.hold(line.trim_end().to_owned());
            }
        }
    }
    fn hold(&mut self, line: String) {
        self.bytes += line.len() + 1;
        self.queue.push_back(line);
        let mut dropped = 0;
        while self.bytes > QUEUE_BYTES || self.queue.len() > QUEUE_EVENTS {
            if let Some(line) = self.queue.pop_front() {
                self.bytes -= line.len() + 1;
                // A frozen failed request must not keep an intentionally evicted
                // event alive. This also runs during journal replay after a crash.
                if serde_json::from_str::<Event>(&line).is_ok_and(|event| {
                    self.state
                        .retry
                        .as_ref()
                        .is_some_and(|batch| !batch.probe && batch.ids.contains(&event.id))
                }) {
                    self.state.retry = None;
                }
                dropped += 1;
            }
        }
        if dropped > 0 {
            log(self.debug, "queue cap reached: dropped oldest event(s)");
        }
    }
    pub fn append(&mut self, event: Event) {
        let Ok(line) = serde_json::to_string(&event) else {
            return;
        };
        let result = match &mut self.file {
            Some(file) => file
                .write_all(line.as_bytes())
                .and_then(|()| file.write_all(b"\n")),
            None => Err(io::Error::other("storage unavailable")),
        };
        self.journal_bytes += line.len() + 1;
        self.hold(line);
        if result.is_err() || self.journal_bytes > 2 * QUEUE_BYTES {
            self.checkpoint();
        }
    }
    pub fn contains(&self, name: &str) -> bool {
        self.queue
            .iter()
            .any(|s| serde_json::from_str::<Event>(s).is_ok_and(|e| e.n == name))
    }
    pub fn remove(&mut self, ids: &[String]) {
        self.queue
            .retain(|s| !serde_json::from_str::<Event>(s).is_ok_and(|e| ids.contains(&e.id)));
        self.bytes = self.queue.iter().map(|s| s.len() + 1).sum();
    }
    pub fn checkpoint(&mut self) -> bool {
        if self.save().is_err() {
            log(self.debug, "storage unavailable; continuing in memory");
            return false;
        }
        self.transition_pending = false;
        true
    }
    /// Baseline and event are replaced together; never append the event to the old
    /// checkpoint, which could otherwise rediscover the transition after a crash.
    pub fn observe_version(&mut self, version: &str, event: Option<Event>) {
        let line = match event.map(|event| serde_json::to_string(&event)).transpose() {
            Ok(line) => line,
            Err(_) => return,
        };
        self.state.last_app_version = version.into();
        if let Some(line) = line {
            self.hold(line);
            self.transition_pending = true;
        }
        self.checkpoint();
    }
    fn save(&mut self) -> io::Result<()> {
        let Some(dir) = &self.dir else {
            return Err(io::Error::other("no state directory"));
        };
        fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        let mut temp = tempfile::Builder::new()
            .prefix(".jelto-")
            .tempfile_in(dir)?;
        serde_json::to_writer(&mut temp, &self.state)?;
        temp.write_all(b"\n")?;
        for line in &self.queue {
            temp.write_all(line.as_bytes())?;
            temp.write_all(b"\n")?;
        }
        temp.as_file().sync_all()?;
        #[cfg(test)]
        if self.fail_checkpoint == Some(false) {
            return Err(io::Error::other(
                "interrupted before checkpoint replacement",
            ));
        }
        self.file = None; // Windows cannot replace an open file without delete sharing.
        temp.persist(dir.join("state.jsonl"))
            .map_err(|err| err.error)?;
        #[cfg(test)]
        if self.fail_checkpoint == Some(true) {
            return Err(io::Error::other("interrupted after checkpoint replacement"));
        }
        #[cfg(unix)]
        File::open(dir)?.sync_all()?;
        self.file = Some(
            OpenOptions::new()
                .append(true)
                .open(dir.join("state.jsonl"))?,
        );
        self.journal_bytes = self.bytes;
        Ok(())
    }
    pub fn clear(&mut self) {
        self.file = None;
        self.state = State::default();
        self.queue = VecDeque::new();
        self.bytes = 0;
        self.journal_bytes = 0;
        self.transition_pending = false;
    }
    pub fn wipe(&mut self) {
        self.clear();
        if let Some(dir) = &self.dir {
            if let Err(err) = fs::remove_dir_all(dir) {
                if err.kind() != io::ErrorKind::NotFound {
                    log(self.debug, "storage wipe failed");
                }
            }
        }
    }
    #[cfg(any(test, feature = "conformance"))]
    pub fn export(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(&self.state).unwrap_or_default();
        if let Some(obj) = value.as_object_mut() {
            obj.remove("retry");
            obj.remove("key");
            obj.remove("app");
        }
        value["queue"] = serde_json::json!({"bytes":self.bytes,"events":self.queue.iter().filter_map(|s| {
            serde_json::from_str::<Event>(s).ok().map(|e| serde_json::json!({"id":e.id,"n":e.n,"t":e.t}))
        }).collect::<Vec<_>>()});
        value
    }
    #[cfg(any(test, feature = "conformance"))]
    pub fn accounted_bytes(&self) -> usize {
        // Queue string capacities, container storage, state strings/tree nodes and frozen
        // request copies. Runtime allocator arenas, TLS and OS page cache are not SDK state.
        std::mem::size_of_val(self)
            + self.queue.capacity() * std::mem::size_of::<String>()
            + self.queue.iter().map(String::capacity).sum::<usize>()
            + serde_json::to_vec(&self.state).map_or(0, |s| s.len()) * 3
            + self.state.install_props.len() * 128
    }
    #[cfg(test)]
    pub fn interrupt_checkpoint(&mut self, after_replace: Option<bool>) {
        self.fail_checkpoint = after_replace;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Metadata, PropValue, Props};
    use num_bigint::BigInt;

    #[test]
    fn transition_checkpoint_recovers_all_or_nothing_with_the_same_event_id() {
        for after_replace in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::new(Some(dir.path().into()), false);
            store.state.install_id = uuid::Uuid::new_v4().to_string();
            store.state.last_app_version = "build A".into();
            store.state.install_claimed = true;
            assert!(store.checkpoint());
            let old_id = store.state.install_id.clone();
            let mut event = Event::new(
                "app_updated",
                &BigInt::from(123),
                Props::from([
                    ("from_version".into(), PropValue::String("build A".into())),
                    ("to_version".into(), PropValue::String("build B".into())),
                ]),
                false,
            );
            event.metadata =
                Some(Metadata::detect("build B", Some("tauri/0.1.0".into())).snapshot(&None));
            let event_id = event.id.clone();
            store.interrupt_checkpoint(Some(after_replace));
            store.observe_version("build B", Some(event));
            assert!(store.transition_pending);
            drop(store); // abrupt exit: no cleanup checkpoint

            let mut recovered = Store::new(Some(dir.path().into()), false);
            recovered.load();
            assert_eq!(recovered.state.install_id, old_id);
            assert!(recovered.state.install_claimed);
            if after_replace {
                assert_eq!(recovered.state.last_app_version, "build B");
                assert_eq!(recovered.queue.len(), 1);
                let recovered_event: Event = serde_json::from_str(&recovered.queue[0]).unwrap();
                assert_eq!(recovered_event.id, event_id);
                assert!(recovered_event.metadata.is_some());
            } else {
                assert_eq!(recovered.state.last_app_version, "build A");
                assert!(recovered.queue.is_empty());
            }
        }
    }

    #[test]
    fn evicted_transition_is_removed_from_retry_in_memory_and_after_journal_replay() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(Some(dir.path().into()), false);
        store.state.install_id = uuid::Uuid::new_v4().to_string();
        store.state.last_app_version = "A".into();
        assert!(store.checkpoint());
        let event = Event::new("app_updated", &BigInt::from(1), Props::new(), false);
        let id = event.id.clone();
        store.observe_version("B", Some(event));
        store.state.retry = Some(Batch {
            ids: vec![id],
            body: "{}".into(),
            ..Batch::default()
        });
        assert!(store.checkpoint());
        for i in 0..QUEUE_EVENTS {
            store.append(Event::new("later", &BigInt::from(i), Props::new(), false));
        }
        assert_eq!(store.queue.len(), QUEUE_EVENTS);
        assert!(store.state.retry.is_none());
        drop(store); // checkpoint still has the old retry; replay retires it too
        let mut recovered = Store::new(Some(dir.path().into()), false);
        recovered.load();
        assert_eq!(recovered.state.last_app_version, "B");
        assert!(!recovered.contains("app_updated"));
        assert!(recovered.state.retry.is_none());
    }
}
