//! Node-side idempotency and acknowledgements of channel commands (multi-Core step 4).
//!
//! The Core keeps durable commands in an outbox and redelivers the idempotent ones that were
//! not acknowledged in time (a lost response, a takeover by another Core). The Node therefore
//! remembers the last [`REMEMBERED_COMMANDS`] command ids it handled, never runs one twice, and
//! acknowledges every handled command in its next control poll, duplicates included, so that
//! the Core stops redelivering it.
//!
//! The memory is per process. A redelivery after a restart of the Node runs again; the work it
//! started before the restart died with that process, and the semantic guards stay in place
//! (`register_scan` refuses a scan or task id already running, results are fenced by attempt on
//! the Core). Persisting the list would cost one synchronous write per command.
use media_contracts::protocol::{
    MAX_ACKED_COMMAND_IDS, MAX_COMMAND_ID_LEN, NodeCommandEnvelope, NodeCommandType,
};
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

/// Command ids remembered to ignore a redelivery.
pub const REMEMBERED_COMMANDS: usize = 512;

#[derive(Default)]
struct Inner {
    recent: VecDeque<String>,
    seen: HashSet<String>,
    pending_acks: VecDeque<String>,
}

/// Recently handled command ids and the acknowledgements still to send.
#[derive(Default)]
pub struct CommandLog(Mutex<Inner>);

impl CommandLog {
    /// Whether `cmd` was already handled and must be skipped (still acknowledged). Only outbox
    /// deliveries are deduplicated: they carry an epoch and a unique id. A Core without the
    /// outbox (no epoch) reuses deterministic ids for new commands (`sync-{library}`,
    /// `cancel-{scan}`), so its repeats must run again, as they always did.
    pub fn already_handled(&self, cmd: &NodeCommandEnvelope) -> bool {
        cmd.command_type != NodeCommandType::Ping
            && cmd.epoch.is_some()
            && !self.admit(&cmd.command_id)
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Record `command_id` as handled now. `false` means it was already handled: the caller
    /// must not run it again. Both cases are acknowledged on the next poll.
    pub fn admit(&self, command_id: &str) -> bool {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let first = !inner.seen.contains(command_id);
        if first {
            inner.seen.insert(command_id.to_owned());
            inner.recent.push_back(command_id.to_owned());
            while inner.recent.len() > REMEMBERED_COMMANDS {
                if let Some(oldest) = inner.recent.pop_front() {
                    inner.seen.remove(&oldest);
                }
            }
        }
        push_ack(&mut inner, command_id);
        first
    }

    /// Acknowledge a command that is not run (delivered after its deadline): the Core must not
    /// redeliver it either.
    pub fn acknowledge(&self, command_id: &str) {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        push_ack(&mut inner, command_id);
    }

    /// Acknowledgements to put in the next poll, oldest first.
    pub fn pending_acks(&self) -> Vec<String> {
        let inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        inner.pending_acks.iter().cloned().collect()
    }

    /// The poll carrying `sent` was accepted by the Core: forget those acknowledgements.
    /// Acknowledgements added meanwhile stay pending.
    pub fn acknowledged(&self, sent: &[String]) {
        if sent.is_empty() {
            return;
        }
        let sent: HashSet<&str> = sent.iter().map(String::as_str).collect();
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        inner.pending_acks.retain(|id| !sent.contains(id.as_str()));
    }
}

fn push_ack(inner: &mut Inner, command_id: &str) {
    // An id the Core would refuse is never sent; a redelivery of it is still deduplicated.
    if command_id.is_empty()
        || command_id.len() > MAX_COMMAND_ID_LEN
        || inner.pending_acks.iter().any(|id| id == command_id)
    {
        return;
    }
    inner.pending_acks.push_back(command_id.to_owned());
    // Bounded: the oldest acknowledgement is dropped; that command may be redelivered, and is
    // then ignored as a duplicate and acknowledged again.
    while inner.pending_acks.len() > MAX_ACKED_COMMAND_IDS {
        inner.pending_acks.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_outbox_deliveries_are_deduplicated() {
        let log = CommandLog::new();
        let envelope = |epoch: Option<i64>| NodeCommandEnvelope {
            command_id: "sync-library".into(),
            command_type: NodeCommandType::SyncInventory,
            expires_at: None,
            epoch,
            payload: serde_json::Value::Null,
        };
        // A released Core reuses `sync-{library}` for every new snapshot: each one runs.
        assert!(!log.already_handled(&envelope(None)));
        assert!(!log.already_handled(&envelope(None)));
        // An outbox redelivery of the same command runs once.
        let mut outbox = envelope(Some(7));
        outbox.command_id = "sync-library-0f3c".into();
        assert!(!log.already_handled(&outbox));
        assert!(log.already_handled(&outbox));
    }

    #[test]
    fn a_redelivered_command_runs_once_and_is_acknowledged_each_time() {
        let log = CommandLog::new();
        assert!(log.admit("scan-1"));
        assert!(log.admit("sync-2"));
        assert_eq!(log.pending_acks(), ["scan-1", "sync-2"]);
        // The poll carrying the acknowledgements is accepted.
        log.acknowledged(&log.pending_acks());
        assert!(log.pending_acks().is_empty());
        // The Core redelivers scan-1 (its acknowledgement arrived too late): ignored, acked.
        assert!(!log.admit("scan-1"));
        assert_eq!(log.pending_acks(), ["scan-1"]);
        // A poll that failed keeps them; an ack added meanwhile is kept by the next success.
        let sent = log.pending_acks();
        log.acknowledge("expired-3");
        log.acknowledged(&sent);
        assert_eq!(log.pending_acks(), ["expired-3"]);
        // The same id is never listed twice.
        log.acknowledge("expired-3");
        assert_eq!(log.pending_acks(), ["expired-3"]);
    }

    #[test]
    fn memory_and_acknowledgements_are_bounded() {
        let log = CommandLog::new();
        for index in 0..REMEMBERED_COMMANDS + 10 {
            assert!(log.admit(&format!("c{index}")));
        }
        assert_eq!(log.pending_acks().len(), MAX_ACKED_COMMAND_IDS);
        assert_eq!(
            log.pending_acks().last().map(String::as_str),
            Some(format!("c{}", REMEMBERED_COMMANDS + 9).as_str())
        );
        // The most recent ids are remembered, the oldest ones forgotten.
        assert!(!log.admit(&format!("c{}", REMEMBERED_COMMANDS + 9)));
        assert!(!log.admit("c10"));
        assert!(
            log.admit("c0"),
            "forgotten after {REMEMBERED_COMMANDS} newer ids"
        );
        // An id the Core would refuse is deduplicated but never sent.
        let long = "x".repeat(MAX_COMMAND_ID_LEN + 1);
        log.acknowledged(&log.pending_acks());
        assert!(log.admit(&long));
        assert!(!log.admit(&long));
        assert!(log.pending_acks().is_empty());
    }
}
