//! Required geometry against finite retained no-contact sweeps, without a stock plane.
use super::{request::EmptySpace, surface};
use crate::object_map::{
    positional::{cover, geometry::*, mesh::Triangle},
    Error,
};
use std::sync::Arc;
pub const COLUMNS: &str = "miss_capture,miss_sequence,miss_path_distance_mm,miss_eroded_radius_mm,miss_signed_separation_mm,miss_relation,miss_conflict_count";

pub struct Conflict {
    pub patch: usize,
    pub contact: usize,
    pub surface: V,
    pub normal: V,
}
pub struct Source {
    pub sweep: surface::no_contact::Sweep,
    pub conflicts: Vec<Conflict>,
}
impl Source {
    pub fn conflicts_json(
        &self,
        samples: &[crate::object_map::positional::probe::Sample],
    ) -> String {
        use super::super::report::reference;
        self.conflicts.iter().map(|c| format!("{{\"patch_source\":{},\"contact_source\":{},\"contact_surface_machine_mm\":{},\"fitted_outward_normal\":{}}}",reference(&samples[c.patch]),reference(&samples[c.contact]),json(c.surface),json(c.normal))).collect::<Vec<_>>().join(",")
    }
}
pub struct Run {
    pub regions: Vec<Vec<Overlap>>,
    pub sources: Vec<Arc<Source>>,
}
pub struct Overlap {
    pub source: Arc<Source>,
    pub distance: f64,
    /// Distance to the finite centre path minus the already eroded radius.
    pub separation: f64,
}
impl Overlap {
    pub fn csv(&self) -> String {
        format!(
            "{},{},{},{},{},{},{}",
            self.source.sweep.capture.as_str(),
            self.source.sweep.sequence,
            self.distance,
            self.source.sweep.radius,
            self.separation,
            self.relation(),
            self.source.conflicts.len()
        )
    }
    pub fn json(&self, samples: &[crate::object_map::positional::probe::Sample]) -> String {
        let conflicts = self.source.conflicts_json(samples);
        format!("{{\"retained_sweep\":{},\"relation\":\"{}\",\"distance_to_finite_center_path_mm\":{},\"signed_separation_mm\":{},\"contact_conflicts\":[{conflicts}]}}",self.source.sweep.json(),self.relation(),self.distance,self.separation)
    }
    pub fn relation(&self) -> &'static str {
        if !self.source.conflicts.is_empty() {
            "contact-no-contact-model-conflict"
        } else if self.separation < 0. {
            "required-fragment-intersects-eroded-sweep-interior"
        } else {
            "required-fragment-on-eroded-sweep-boundary"
        }
    }
}
pub fn run(
    cover: &[cover::Sample],
    local: &[surface::local::Local<'_>],
    misses: &[surface::no_contact::Sweep],
    sr: &surface::request::Request,
    pose: Pose,
    policy: EmptySpace,
) -> Result<Run, Error> {
    let Some(comparisons) = policy.comparisons() else {
        return Ok(Run {
            regions: cover.iter().map(|_| Vec::new()).collect(),
            sources: Vec::new(),
        });
    };
    if matches!(sr.no_contact, surface::request::NoContactModel::Legacy) {
        return Err(Error::Input("This material request requires the source surface's explicit no-contact probe/error model. Prepare a new surface analysis with its retained miss selection and evidence-based allowance, then reassess; no empty-space model was inferred from live configuration.".into()));
    }
    if misses
        .len()
        .checked_mul(cover.len())
        .is_none_or(|n| n > comparisons)
    {
        return Err(Error::Input("max_no_contact_comparisons cannot cover every retained sweep and required triangle fragment. Increase this computation budget or choose another explicit cover radius; no pairs or geometry were omitted.".into()));
    }
    let sources = misses.iter().map(|sweep| {
        let mut conflicts = Vec::new();
        for l in local {
            for c in &l.support.conflicts {
                let original = l.station.no_contact.get(c.miss).ok_or_else(|| Error::Data("A retained surface conflict lost its no-contact source. Preserve the analysis and recalculate from its original captures; no conflicting sweep was accepted as empty space.".into()))?;
                if original.capture == sweep.capture && original.sequence == sweep.sequence {
                    conflicts.push(Conflict { patch: l.station.seed, contact: c.contact, surface: c.surface, normal: l.patch.normal });
                }
            }
        }
        Ok(Arc::new(Source { sweep: sweep.clone(), conflicts }))
    }).collect::<Result<Vec<_>, Error>>()?;
    let mut result = Vec::with_capacity(cover.len());
    for sample in cover {
        let v = sample.vertices.map(|p| pose.point(p));
        let normal = cross(sub(v[1], v[0]), sub(v[2], v[0]));
        let length = norm(normal);
        if !v.into_iter().all(finite) || !length.is_finite() || length == 0. {
            return Err(Error::Data("A required triangle fragment became nonfinite or degenerate after the candidate transform. Inspect the retained mesh, placement and cover scale; no empty-space classification was supplied for collapsed geometry.".into()));
        }
        let triangle = Triangle {
            v,
            n: normal.map(|x| x / length),
        };
        let mut overlaps = Vec::new();
        for source in &sources {
            let distance = source.sweep.triangle_distance(triangle)?;
            let separation = distance - source.sweep.radius;
            if !separation.is_finite() {
                return Err(Error::Data("No-contact separation overflowed. Inspect the retained probe model and coordinate scale before reassessing; no empty region was inferred.".into()));
            }
            if separation <= 0. {
                overlaps.push(Overlap {
                    source: source.clone(),
                    distance,
                    separation,
                });
            }
        }
        result.push(overlaps);
    }
    Ok(Run {
        regions: result,
        sources,
    })
}
