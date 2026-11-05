use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const MAX_CHAT_LENGTH: usize = 500;
pub(super) const MAX_STORED_MESSAGES: usize = 100;
pub(super) const BROADCAST_CAPACITY: usize = 128;
pub(super) const DEFAULT_INVITE_EXPIRY_SECONDS: i64 = 86400; // 24 hours

#[inline]
pub(super) fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[inline]
pub(super) fn current_epoch_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
