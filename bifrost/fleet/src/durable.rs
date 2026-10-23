//! The durable, integrity-protected [`DurableFleet`].
//!
//! Fleet state (channel desires, installation actuals and agent check-ins) is persisted
//! as a KMS-sealed, append-only journal of mutations (`SetChannel` / `Register` /
//! `MarkUpdated` / `CheckIn`), each framed, encrypted-and-authenticated at rest via a
//! [`Sealer`] (`kms::KmsSealer` — AES-256-GCM under ML-KEM-1024), `fsync`ed, with
//! `0600` perms. On [`DurableFleet::open`] the journal is decrypted and **replayed**
//! into an in-memory [`Fleet`]; a tampered record fails AEAD authentication and the
//! open fails closed — so on restart channels and check-ins are recovered and
//! re-verified. [FR-B-3] [RC-12]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use bifrost_rollout::VersionId;

use crate::{AgentReport, Fleet, Installation};

const JOURNAL_DS: &[u8] = b"midgard-fleet-state-v1";
const OP_SET_CHANNEL: u8 = 0;
const OP_REGISTER: u8 = 1;
const OP_MARK_UPDATED: u8 = 2;
const OP_CHECK_IN: u8 = 3;

/// An error persisting or recovering the durable fleet.
#[derive(Debug)]
pub struct FleetIoError(pub String);

impl std::fmt::Display for FleetIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "durable fleet i/o: {}", self.0)
    }
}
impl std::error::Error for FleetIoError {}

/// A [`Fleet`] whose channel desires and installation records survive a restart.
pub struct DurableFleet {
    inner: Fleet,
    file: File,
    path: PathBuf,
    sealer: Box<dyn Sealer>,
    fleet_id: String,
    op_seq: u64,
}

impl DurableFleet {
    /// Open (creating if absent) the fleet journal at `path`, decrypting and
    /// replaying every mutation into memory. Fails closed on a tampered/corrupt
    /// record.
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        fleet_id: impl Into<String>,
    ) -> Result<Self, FleetIoError> {
        let path = path.as_ref().to_path_buf();
        let fleet_id = fleet_id.into();
        let mut inner = Fleet::new();
        let mut op_seq = 0u64;

        if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)
                .map_err(io)?
                .read_to_end(&mut buf)
                .map_err(io)?;
            let mut pos = 0usize;
            while pos < buf.len() {
                if pos + 4 > buf.len() {
                    return Err(FleetIoError("truncated frame header".into()));
                }
                let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                    as usize;
                pos += 4;
                let end = pos
                    .checked_add(len)
                    .filter(|&e| e <= buf.len())
                    .ok_or_else(|| FleetIoError("truncated frame body".into()))?;
                let plaintext = sealer
                    .unseal(&aad(&fleet_id, op_seq), &buf[pos..end])
                    .map_err(|e| FleetIoError(e.to_string()))?;
                pos = end;
                apply(&mut inner, &decode_op(&plaintext)?);
                op_seq += 1;
            }
        }

        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        // On Windows the file inherits its folder's ACL: the app's working copy, %LOCALAPPDATA%\Midgard, is private to the user (the W2 shell checks it).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path).map_err(io)?;

        Ok(Self {
            inner,
            file,
            path,
            sealer,
            fleet_id,
            op_seq,
        })
    }

    /// Declare (or update) a channel's desired version, durably.
    pub fn set_channel(
        &mut self,
        name: impl Into<String>,
        desired: VersionId,
    ) -> Result<(), FleetIoError> {
        let name = name.into();
        let mut b = vec![OP_SET_CHANNEL];
        put_str(&mut b, &name);
        put_str(&mut b, &desired.0);
        self.journal(&b)?;
        self.inner.set_channel(name, desired);
        Ok(())
    }

    /// Register or replace an installation record, durably.
    pub fn register(&mut self, install: Installation) -> Result<(), FleetIoError> {
        let mut b = vec![OP_REGISTER];
        put_str(&mut b, &install.id);
        put_str(&mut b, &install.channel);
        put_str(&mut b, &install.version.0);
        self.journal(&b)?;
        self.inner.register(install);
        Ok(())
    }

    /// Record that an installation now runs `version` (a check-in), durably.
    pub fn mark_updated(
        &mut self,
        install_id: &str,
        version: VersionId,
    ) -> Result<(), FleetIoError> {
        let mut b = vec![OP_MARK_UPDATED];
        put_str(&mut b, install_id);
        put_str(&mut b, &version.0);
        self.journal(&b)?;
        self.inner.mark_updated(install_id, version);
        Ok(())
    }

    /// Record an agent's check-in, stamped with the manager's clock `now`, durably:
    /// after a restart the installation's reported version, health and last-seen time
    /// are all recovered. [FR-B-3] [RC-17]
    pub fn check_in(&mut self, report: AgentReport, now: u64) -> Result<(), FleetIoError> {
        let mut b = vec![OP_CHECK_IN];
        put_str(&mut b, &report.install_id);
        put_str(&mut b, &report.channel);
        put_str(&mut b, &report.version.0);
        b.push(u8::from(report.healthy));
        b.extend_from_slice(&now.to_le_bytes());
        self.journal(&b)?;
        self.inner.check_in(report, now);
        Ok(())
    }

    /// The recovered in-memory fleet (drift/reconcile/read surface).
    pub fn fleet(&self) -> &Fleet {
        &self.inner
    }
    /// The journal path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn journal(&mut self, plaintext: &[u8]) -> Result<(), FleetIoError> {
        let sealed = self
            .sealer
            .seal(&aad(&self.fleet_id, self.op_seq), plaintext)
            .map_err(|e| FleetIoError(e.to_string()))?;
        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        self.file.write_all(&frame).map_err(io)?;
        self.file.sync_all().map_err(io)?;
        self.op_seq += 1;
        Ok(())
    }
}

fn io(e: std::io::Error) -> FleetIoError {
    FleetIoError(e.to_string())
}

fn aad(fleet_id: &str, seq: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(JOURNAL_DS.len() + 4 + fleet_id.len() + 8);
    a.extend_from_slice(JOURNAL_DS);
    a.extend_from_slice(&(fleet_id.len() as u32).to_le_bytes());
    a.extend_from_slice(fleet_id.as_bytes());
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

// --- codec ----------------------------------------------------------------

enum Op {
    SetChannel(String, VersionId),
    Register(Installation),
    MarkUpdated(String, VersionId),
    CheckIn(AgentReport, u64),
}

fn apply(fleet: &mut Fleet, op: &Op) {
    match op {
        Op::SetChannel(name, v) => fleet.set_channel(name.clone(), v.clone()),
        Op::Register(i) => fleet.register(i.clone()),
        Op::MarkUpdated(id, v) => fleet.mark_updated(id, v.clone()),
        Op::CheckIn(report, now) => fleet.check_in(report.clone(), *now),
    }
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, FleetIoError> {
        let b = self.take(1)?;
        Ok(b[0])
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], FleetIoError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| FleetIoError("codec truncated".into()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn string(&mut self) -> Result<String, FleetIoError> {
        let b = self.take(4)?;
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| FleetIoError("codec invalid utf8".into()))
    }
    fn u64(&mut self) -> Result<u64, FleetIoError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
}

fn decode_op(b: &[u8]) -> Result<Op, FleetIoError> {
    let mut r = Reader { b, pos: 0 };
    let op = match r.u8()? {
        OP_SET_CHANNEL => Op::SetChannel(r.string()?, VersionId(r.string()?)),
        OP_REGISTER => Op::Register(Installation {
            id: r.string()?,
            channel: r.string()?,
            version: VersionId(r.string()?),
        }),
        OP_MARK_UPDATED => Op::MarkUpdated(r.string()?, VersionId(r.string()?)),
        OP_CHECK_IN => {
            let install_id = r.string()?;
            let channel = r.string()?;
            let version = VersionId(r.string()?);
            let healthy = match r.u8()? {
                0 => false,
                1 => true,
                h => return Err(FleetIoError(format!("invalid health flag {h}"))),
            };
            let now = r.u64()?;
            Op::CheckIn(
                AgentReport {
                    install_id,
                    channel,
                    version,
                    healthy,
                },
                now,
            )
        }
        t => return Err(FleetIoError(format!("unknown op tag {t}"))),
    };
    if r.pos != b.len() {
        return Err(FleetIoError("codec trailing bytes".into()));
    }
    Ok(op)
}
