//! The last observation sequence admitted from each feed, sealed at rest so replay
//! protection survives a restart. The feed gateway's [`ReplayGuard`] refuses any
//! sequence not above a feed's last one; kept only in memory, a restart would forget
//! it and admit once more every captured observation still inside the freshness
//! window, which is seven days by default. The file is small (one number per feed)
//! and rewritten atomically before each admitted observation is fused.
//! [VLD-ANB-MIDGARD-0041] [FR-H-3] [RC-13]

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use heimdall_feed_gateway::ReplayGuard;
use heimdall_picture::FeedId;
use heimdall_store::StoreError;

/// Domain separation for the sealed sequence file.
const AAD: &[u8] = b"midgard-heimdall-feed-sequences-v1";
/// The plaintext layout version.
const VERSION: u8 = 1;

/// The sealed file of each feed's last admitted sequence.
pub struct SequenceFile {
    path: PathBuf,
    sealer: Box<dyn Sealer>,
}

impl SequenceFile {
    /// Open the file at `path` (absent means no feed has a sequence yet) and return it
    /// with the sequences it holds. Fails closed on tamper or corruption: a service
    /// that cannot read its replay state does not start without it.
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
    ) -> Result<(Self, BTreeMap<FeedId, u64>), StoreError> {
        let path = path.as_ref().to_path_buf();
        let sequences = if path.exists() {
            let sealed = fs::read(&path).map_err(io)?;
            let plaintext = sealer
                .unseal(AAD, &sealed)
                .map_err(|e| StoreError::Seal(e.to_string()))?;
            decode(&plaintext)?
        } else {
            BTreeMap::new()
        };
        Ok((Self { path, sealer }, sequences))
    }

    /// Seal and write the guard's current sequences, replacing the file atomically.
    pub fn save(&self, guard: &ReplayGuard) -> Result<(), StoreError> {
        let sealed = self
            .sealer
            .seal(AAD, &encode(&guard.last_sequences()))
            .map_err(|e| StoreError::Seal(e.to_string()))?;
        let tmp = self.path.with_extension("tmp");
        {
            let mut opts = OpenOptions::new();
            opts.create(true).write(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp).map_err(io)?;
            f.write_all(&sealed).map_err(io)?;
            f.sync_all().map_err(io)?;
        }
        fs::rename(&tmp, &self.path).map_err(io)
    }
}

fn io(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

fn encode(sequences: &BTreeMap<FeedId, u64>) -> Vec<u8> {
    let mut out = vec![VERSION];
    out.extend_from_slice(&(sequences.len() as u32).to_le_bytes());
    for (feed, sequence) in sequences {
        out.extend_from_slice(&(feed.0.len() as u32).to_le_bytes());
        out.extend_from_slice(feed.0.as_bytes());
        out.extend_from_slice(&sequence.to_le_bytes());
    }
    out
}

/// The next `n` bytes of `rest`, advancing it; fails closed when short.
fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], StoreError> {
    if rest.len() < n {
        return Err(StoreError::Decode("feed sequences truncated".into()));
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_u32(rest: &mut &[u8]) -> Result<usize, StoreError> {
    let s = take(rest, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

fn decode(b: &[u8]) -> Result<BTreeMap<FeedId, u64>, StoreError> {
    let mut rest = b;
    if take(&mut rest, 1)? != [VERSION] {
        return Err(StoreError::Decode(
            "unsupported feed sequences version".into(),
        ));
    }
    let count = take_u32(&mut rest)?;
    let mut sequences = BTreeMap::new();
    for _ in 0..count {
        let len = take_u32(&mut rest)?;
        let feed = String::from_utf8(take(&mut rest, len)?.to_vec())
            .map_err(|_| StoreError::Decode("feed id is not utf-8".into()))?;
        let mut n = [0u8; 8];
        n.copy_from_slice(take(&mut rest, 8)?);
        sequences.insert(FeedId(feed), u64::from_le_bytes(n));
    }
    if !rest.is_empty() {
        return Err(StoreError::Decode("feed sequences trailing bytes".into()));
    }
    Ok(sequences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_round_trip_and_fail_closed() {
        let mut sequences = BTreeMap::new();
        sequences.insert(FeedId("radar-1".into()), 7);
        sequences.insert(FeedId("sonar".into()), u64::MAX);
        let bytes = encode(&sequences);
        assert_eq!(decode(&bytes).unwrap(), sequences);
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        let mut other = bytes;
        other[0] = 2;
        assert!(decode(&other).is_err());
    }
}
