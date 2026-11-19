//! `HeimdallMotion`: the labelled synthetic hour of track positions the demo dataset
//! carries (`heimdall.motion`). Written by hand: the TypeScript contract has no motion
//! type, because the web console animates its tracks itself; the rows are made by
//! `clients/demo/tools/extract-fixtures.mjs`. `[AC-C-13]` `[FR-C-1]`

use attest::json::Json;

use crate::consoles::decode::{field, object, DecodeError, FromJson};

/// One position of a track, `t` seconds into the hour.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionSample {
    /// Seconds from the start of the hour.
    pub t: u32,
    /// Latitude, degrees, within [-90, 90].
    pub lat: f64,
    /// Longitude, degrees, within [-180, 180].
    pub lon: f64,
}

/// The motion of one track over the synthetic hour.
#[derive(Debug, Clone, PartialEq)]
pub struct HeimdallMotion {
    /// The track id (`HeimdallTrack.id`).
    pub id: String,
    /// Where the positions come from, e.g. `synthetic: HeimdallMock track, closed loop`.
    pub provenance: String,
    /// Positions in strictly increasing `t`.
    pub samples: Vec<MotionSample>,
}

fn bounded(o: &[(String, Json)], key: &str, path: &str, limit: f64) -> Result<f64, DecodeError> {
    let x: f64 = field(o, key, path)?;
    if (-limit..=limit).contains(&x) {
        Ok(x)
    } else {
        Err(DecodeError {
            path: format!("{path}.{key}"),
            reason: format!("outside [-{limit}, {limit}]"),
        })
    }
}

impl FromJson for MotionSample {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        let o = object(v, path, &["t", "lat", "lon"])?;
        Ok(Self {
            t: field(o, "t", path)?,
            lat: bounded(o, "lat", path, 90.0)?,
            lon: bounded(o, "lon", path, 180.0)?,
        })
    }
}

impl FromJson for HeimdallMotion {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        let o = object(v, path, &["id", "provenance", "samples"])?;
        let samples: Vec<MotionSample> = field(o, "samples", path)?;
        if let Some(i) = (1..samples.len()).find(|&i| samples[i].t <= samples[i - 1].t) {
            return Err(DecodeError {
                path: format!("{path}.samples[{i}].t"),
                reason: "not after the sample before it".into(),
            });
        }
        Ok(Self {
            id: field(o, "id", path)?,
            provenance: field(o, "provenance", path)?,
            samples,
        })
    }
}
