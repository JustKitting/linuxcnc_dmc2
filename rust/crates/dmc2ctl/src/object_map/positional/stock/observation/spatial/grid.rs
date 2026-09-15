//! Unique XY cells prevent required-mesh tessellation density weighting samples.
use crate::{object_map::Error, probe_data::mapper_settings::Settings};

pub type Key = [i64; 2];
pub struct Grid {
    pub origin: [f64; 2],
    pub spacing: f64,
    pub min: [f64; 2],
    pub max: [f64; 2],
}
fn arithmetic() -> Error {
    Error::Input("Spatial grid arithmetic cannot represent distinct cell centres at this coordinate scale. Inspect units and choose a representable sampling spacing before retrying; no coordinates were rounded into substitute targets.".into())
}
fn index(v: f64) -> Result<i64, Error> {
    // Require both the integer and its half-cell centre to be representable.
    if !v.is_finite()
        || v < i64::MIN as f64
        || v >= i64::MAX as f64
        || (v as i64) as f64 != v
        || (v + 0.5) - v != 0.5
    {
        return Err(arithmetic());
    }
    Ok(v as i64)
}
impl Grid {
    pub fn new(s: &Settings, spacing: f64) -> Result<Self, Error> {
        if !spacing.is_finite() || spacing < s.step[0].max(s.step[1]) {
            return Err(Error::Input("sample_spacing_mm must be at least the largest retained XY step. Choose a resolvable sampling interval and retry.".into()));
        }
        Ok(Self {
            origin: [s.origin[0], s.origin[1]],
            spacing,
            min: [s.min[0], s.min[1]],
            max: [s.max[0], s.max[1]],
        })
    }
    pub fn xy(&self, key: Key) -> Result<[f64; 2], Error> {
        let mut p = [0.; 2];
        for i in 0..2 {
            let k = key[i] as f64;
            index(k)?;
            p[i] = self.origin[i] + (k + 0.5) * self.spacing;
            if !p[i].is_finite() || p[i] + self.spacing <= p[i] || p[i] - self.spacing >= p[i] {
                return Err(arithmetic());
            }
        }
        Ok(p)
    }
    /// Candidate centre limits for cells intersecting a projected cover disk.
    /// Cell centres themselves must remain inside the retained travel envelope.
    pub fn range(&self, p: [f64; 2], radius: f64) -> Result<Option<(Key, Key)>, Error> {
        let mut lo = [0; 2];
        let mut hi = [0; 2];
        for i in 0..2 {
            let pad = radius + self.spacing / 2.;
            let lower = p[i] - pad;
            let upper = p[i] + pad;
            if !pad.is_finite() || !lower.is_finite() || !upper.is_finite() {
                return Err(arithmetic());
            }
            let lower = lower.max(self.min[i]);
            let upper = upper.min(self.max[i]);
            if lower > upper {
                return Ok(None);
            }
            let a = ((lower - self.origin[i]) / self.spacing - 0.5).ceil();
            let b = ((upper - self.origin[i]) / self.spacing - 0.5).floor();
            lo[i] = index(a)?;
            hi[i] = index(b)?;
            if lo[i] > hi[i] {
                return Ok(None);
            }
        }
        Ok(Some((lo, hi)))
    }
    pub fn intersects(&self, key: Key, p: [f64; 2], radius: f64) -> Result<bool, Error> {
        let c = self.xy(key)?;
        let d =
            std::array::from_fn::<_, 2, _>(|i| ((c[i] - p[i]).abs() - self.spacing / 2.).max(0.));
        let distance = d[0].hypot(d[1]);
        if !distance.is_finite() {
            return Err(arithmetic());
        }
        Ok(distance <= radius)
    }
}

pub struct Budget {
    remaining: usize,
    pub used: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            used: 0,
        }
    }
    pub fn take(&mut self, count: Option<usize>) -> Result<(), Error> {
        let count = count.filter(|n| *n <= self.remaining).ok_or_else(|| Error::Input("max_candidate_comparisons cannot cover the projected regions and retained sample comparisons. Increase this computation budget or select another spacing/source analysis; no region was silently dropped.".into()))?;
        self.remaining -= count;
        self.used += count;
        Ok(())
    }
}
