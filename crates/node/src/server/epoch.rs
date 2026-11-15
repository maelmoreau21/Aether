//! Command-channel epoch fence (multi-Core, roadmap §2.3).
//!
//! The Core that owns this Node's command channel stamps every delivered command (the ping
//! included) with its ownership epoch, inside the signed response body. The Node keeps the
//! highest epoch it has seen in `node_channel_epoch.json` (atomic write) and refuses any command
//! of a lower epoch: a former owner, cut off from the database and unaware that another Core
//! took the channel over, can no longer act on this Node once the new owner has spoken once.
//! A command without an epoch comes from a Core that predates fencing and is accepted as before.

use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::path::Path;
use std::path::PathBuf;

const FILE: &str = "node_channel_epoch.json";
const PENDING: &str = "node_channel_epoch.pending";
/// Epochs are stamped from a Core's clock (`max(previous + 1, now)`): a legitimate mark is never
/// far ahead of the present. A mark beyond this margin was written under a wrong clock (host
/// clock error later corrected, database restored); it would refuse every legitimate command
/// until the clock catches up, so it is dropped instead. A stale Core's epoch stays below a
/// legitimate mark and is still refused.
const MAX_FUTURE_SECONDS: i64 = 3_600;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    highest_epoch: i64,
}

/// Decision of [`ChannelEpochFence::admit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochVerdict {
    Accepted,
    /// The command comes from an older owner than one already heard from.
    Stale {
        highest: i64,
    },
}

pub struct ChannelEpochFence {
    data_dir: PathBuf,
    /// Held across the persistence of a raised mark, so marks are written in order.
    highest: tokio::sync::Mutex<i64>,
}

impl ChannelEpochFence {
    /// Load the persisted mark. A missing file is a Node that never saw an epoch; an unreadable
    /// one is logged and fails open (0), as a Node without fencing behaved before.
    pub async fn load(data_dir: &Path) -> Self {
        let highest = match tokio::fs::read(data_dir.join(FILE)).await {
            Ok(bytes) => match serde_json::from_slice::<Persisted>(&bytes) {
                Ok(persisted) => persisted.highest_epoch,
                Err(err) => {
                    eprintln!(
                        "{}",
                        serde_json::json!({"event":"node_channel_epoch_unreadable","error":err.to_string()})
                    );
                    0
                }
            },
            Err(_) => 0,
        };
        Self {
            data_dir: data_dir.to_owned(),
            highest: tokio::sync::Mutex::new(highest),
        }
    }

    pub async fn highest(&self) -> i64 {
        *self.highest.lock().await
    }

    /// Accept or refuse a command of `epoch`. A higher epoch raises the mark (persisted before
    /// the command is acted upon); an equal one is the current owner; no epoch is a Core that
    /// predates fencing.
    pub async fn admit(&self, epoch: Option<i64>) -> EpochVerdict {
        self.admit_at(epoch, media_contracts::protocol::unix_now())
            .await
    }

    async fn admit_at(&self, epoch: Option<i64>, now: i64) -> EpochVerdict {
        let Some(epoch) = epoch else {
            return EpochVerdict::Accepted;
        };
        let mut highest = self.highest.lock().await;
        if epoch < *highest && *highest <= now.saturating_add(MAX_FUTURE_SECONDS) {
            return EpochVerdict::Stale { highest: *highest };
        }
        if epoch < *highest {
            eprintln!(
                "{}",
                serde_json::json!({"event":"node_channel_epoch_mark_in_future","highest":*highest,"epoch":epoch})
            );
        }
        if epoch != *highest {
            *highest = epoch;
            if let Err(err) = self.persist(epoch).await {
                // The mark still holds in memory until a restart.
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"node_channel_epoch_persist_failed","error":err.to_string()})
                );
            }
        }
        EpochVerdict::Accepted
    }

    /// Forget the mark: the Node was paired again, possibly with another installation whose
    /// epochs are unrelated to the previous one.
    pub async fn reset(&self) -> io::Result<()> {
        let mut highest = self.highest.lock().await;
        *highest = 0;
        self.persist(0).await
    }

    async fn persist(&self, epoch: i64) -> io::Result<()> {
        let bytes = serde_json::to_vec(&Persisted {
            highest_epoch: epoch,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let temporary = self.data_dir.join(PENDING);
        let mut file = tokio::fs::File::create(&temporary).await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(temporary, self.data_dir.join(FILE)).await?;
        #[cfg(unix)]
        {
            let directory = self.data_dir.clone();
            tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
                .await
                .map_err(io::Error::other)??;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_highest_epoch_survives_a_restart_and_fences_older_owners() {
        let dir = tempfile::tempdir().unwrap();
        let fence = ChannelEpochFence::load(dir.path()).await;
        assert_eq!(fence.highest().await, 0);
        // A Core that predates fencing is accepted and leaves the mark alone.
        assert_eq!(fence.admit(None).await, EpochVerdict::Accepted);
        assert_eq!(fence.admit(Some(10)).await, EpochVerdict::Accepted);
        assert_eq!(fence.admit(Some(10)).await, EpochVerdict::Accepted);
        // A newer owner raises the mark; the former one is refused from then on.
        assert_eq!(fence.admit(Some(12)).await, EpochVerdict::Accepted);
        assert_eq!(
            fence.admit(Some(10)).await,
            EpochVerdict::Stale { highest: 12 }
        );
        assert_eq!(fence.admit(None).await, EpochVerdict::Accepted);
        assert!(!dir.path().join(PENDING).exists(), "written atomically");
        drop(fence);

        let restarted = ChannelEpochFence::load(dir.path()).await;
        assert_eq!(restarted.highest().await, 12);
        assert_eq!(
            restarted.admit(Some(11)).await,
            EpochVerdict::Stale { highest: 12 }
        );
        assert_eq!(restarted.admit(Some(12)).await, EpochVerdict::Accepted);

        // Pairing again forgets the mark, durably.
        restarted.reset().await.unwrap();
        assert_eq!(ChannelEpochFence::load(dir.path()).await.highest().await, 0);
    }

    #[tokio::test]
    async fn a_mark_written_under_a_wrong_clock_does_not_lock_the_node_out() {
        let dir = tempfile::tempdir().unwrap();
        let fence = ChannelEpochFence::load(dir.path()).await;
        let now = 1_800_000_000;
        // A Core with a clock one day ahead stamped this mark.
        assert_eq!(
            fence.admit_at(Some(now + 86_400), now).await,
            EpochVerdict::Accepted
        );
        // Clock fixed, ownership row lost (restored backup): the new owner's epoch is `now`.
        assert_eq!(fence.admit_at(Some(now), now).await, EpochVerdict::Accepted);
        assert_eq!(
            ChannelEpochFence::load(dir.path()).await.highest().await,
            now
        );
        // A legitimate mark still fences a former owner.
        assert_eq!(
            fence.admit_at(Some(now - 5), now).await,
            EpochVerdict::Stale { highest: now }
        );
    }

    #[tokio::test]
    async fn an_unreadable_mark_fails_open() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), b"{not json").unwrap();
        let fence = ChannelEpochFence::load(dir.path()).await;
        assert_eq!(fence.highest().await, 0);
        assert_eq!(fence.admit(Some(3)).await, EpochVerdict::Accepted);
        assert_eq!(ChannelEpochFence::load(dir.path()).await.highest().await, 3);
    }
}
