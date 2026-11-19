//! UniFFI mirror of `HeimdallMotion`, the labelled synthetic hour of track positions
//! (`midgard_client_core::consoles::motion`). Written by hand, as the core type is:
//! the TypeScript contract has no motion type. `[FR-C-1]` `[DD-6]` `[AC-C-13]`

use midgard_client_core::consoles::motion as core;

/// One position of a track, `t` seconds into the hour.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MotionSample {
    /// Seconds from the start of the hour.
    pub t: u32,
    /// Latitude, degrees.
    pub lat: f64,
    /// Longitude, degrees.
    pub lon: f64,
}

/// The motion of one track over the synthetic hour.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HeimdallMotion {
    /// The track id (`HeimdallTrack.id`).
    pub id: String,
    /// Where the positions come from (says "synthetic").
    pub provenance: String,
    /// Positions in strictly increasing `t`.
    pub samples: Vec<MotionSample>,
}

impl From<core::HeimdallMotion> for HeimdallMotion {
    fn from(x: core::HeimdallMotion) -> Self {
        Self {
            id: x.id,
            provenance: x.provenance,
            samples: x
                .samples
                .into_iter()
                .map(|s| MotionSample {
                    t: s.t,
                    lat: s.lat,
                    lon: s.lon,
                })
                .collect(),
        }
    }
}
