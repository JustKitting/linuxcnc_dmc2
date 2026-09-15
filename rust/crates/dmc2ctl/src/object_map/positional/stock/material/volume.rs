//! Connected retained clear sweeps against unchanged required material volume.
use super::{
    empty,
    query::Region,
    request::{EmptySpace, OCCUPANCY},
};
use crate::object_map::{
    positional::{
        geometry::*,
        mesh::{
            solid::{Distance, Shell, Solid},
            Mesh,
        },
        probe::Sample,
    },
    record::quote,
    Error,
};
use std::sync::Arc;

enum Relation {
    BoundaryIntersection,
    BoundaryTouch,
    Inside { model_point: V, distance: Distance },
    Outside { model_point: V, distance: Distance },
}
impl Relation {
    fn name(&self) -> &'static str {
        match self {
            Self::BoundaryIntersection => "sweep-intersects-required-boundary",
            Self::BoundaryTouch => "sweep-touches-required-boundary",
            Self::Inside { .. } => "sweep-wholly-inside-required-material",
            Self::Outside { .. } => "sweep-wholly-outside-required-material",
        }
    }
}
struct Sweep {
    source: Arc<empty::Source>,
    regions: Vec<usize>,
    relation: Relation,
}
impl Sweep {
    fn need(&self) -> Option<(&'static str, &'static str)> {
        if !self.source.conflicts.is_empty() {
            Some(("required-volume-no-contact-conflict", "This retained clear sweep conflicts with original surface contacts. Resolve the source probe/reference model before interpreting its required-material relation; no contradictory observation was discarded."))
        } else {
            match self.relation {
                Relation::Inside { .. } => Some(("required-volume-no-contact-overlap", "A connected eroded clear sweep lies wholly inside the declared required material, without crossing its surface. Review the original miss, physical reference and candidate placement; surface-only clearance cannot resolve this volume contradiction.")),
                _ => None,
            }
        }
    }
    fn json(&self, samples: &[Sample]) -> String {
        let witness = match &self.relation {
            Relation::Inside { model_point, distance } | Relation::Outside { model_point, distance } => format!("{{\"source\":\"retained probe-centre path start\",\"machine_mm\":{},\"model_mm\":{},\"inward_distance_to_required_boundary_mm\":{},\"nearest_required_triangle\":{},\"winding\":{}}}",json(self.source.sweep.center_from),json(*model_point),distance.inward,distance.triangle,distance.winding.map(|v|v.to_string()).unwrap_or_else(||"null".into())),
            _ => "null".into(),
        };
        let evidence = self
            .need()
            .map(|(kind, _)| kind)
            .unwrap_or("retained-geometric-relation");
        format!("{{\"retained_sweep\":{},\"geometric_relation\":{},\"evidence_state\":{},\"required_surface_regions\":{:?},\"witness\":{witness},\"contact_conflicts\":[{}]}}",self.source.sweep.json(),quote(self.relation.name()),quote(evidence),self.regions,self.source.conflicts_json(samples))
    }
}
pub struct Assessment {
    shells: Vec<Shell>,
    topology_visits: usize,
    triangle_pairs: usize,
    vertices: usize,
    structure_winding_terms: usize,
    reserved_winding_terms: usize,
    sweeps: Vec<Sweep>,
}
impl Assessment {
    pub fn json(&self, samples: &[Sample]) -> String {
        let shells = self.shells.iter().map(|s|format!("{{\"first_required_triangle\":{},\"triangle_count\":{},\"signed_volume_model_mm3\":{},\"surrounding_winding\":{}}}",s.first_triangle,s.triangle_count,s.signed_volume,s.surrounding_winding.map(|v|v.to_string()).unwrap_or_else(||"null".into()))).collect::<Vec<_>>().join(",");
        format!("{{\"occupancy_model\":{},\"geometry_role\":\"unchanged-required-operation-material\",\"boundary\":{{\"shells\":[{shells}],\"vertices\":{},\"topology_visits\":{},\"triangle_pairs\":{},\"structure_winding_terms\":{},\"reserved_winding_terms\":{}}},\"sweeps\":[{}],\"interpretation\":\"The declared required boundary is closed, embedded and consistently oriented with material winding one and empty winding zero. Separate exterior bodies and oppositely oriented nested cavities remain explicit. After comparing the entire finite eroded sweep with every required triangle fragment, a boundary-disjoint connected sweep has one inside/outside relation, witnessed by its original centre-path start transformed back to model millimetres. This geometric result uses the retained probe/error model; it does not establish physical registration, measured stock occupancy or CAM readiness.\"}}",quote(OCCUPANCY),self.vertices,self.topology_visits,self.triangle_pairs,self.structure_winding_terms,self.reserved_winding_terms,self.sweeps.iter().map(|s|s.json(samples)).collect::<Vec<_>>().join(","))
    }
    pub fn needs(&self, samples: &[Sample]) -> Vec<String> {
        self.sweeps.iter().filter_map(|s|s.need().map(|(kind,message)|format!("{{\"kind\":{},\"message\":{},\"required_volume_evidence\":{},\"measurement_resolved\":false,\"machine_action_authorized\":false}}",quote(kind),quote(message),s.json(samples)))).collect()
    }
    pub fn continuation(&self, samples: &[Sample]) -> String {
        format!(
            ",\"required_volume_assessment\":{},\"unresolved_required_volume_needs\":[{}]",
            self.json(samples),
            self.needs(samples).join(",")
        )
    }
}
pub fn assess(
    mesh: &Mesh,
    pose: Pose,
    sources: &[Arc<empty::Source>],
    regions: &[Region],
    policy: EmptySpace,
) -> Result<Option<Assessment>, Error> {
    let EmptySpace::RequiredVolume {
        topology_visits,
        winding_terms,
        ..
    } = policy
    else {
        return Ok(None);
    };
    let solid = Solid::required(mesh, topology_visits, winding_terms)?;
    let reserved = sources.len().checked_mul(mesh.triangles().len()).and_then(|n|n.checked_add(solid.structure_winding_terms)).ok_or_else(|| Error::Input("Required-volume winding budget overflows. Inspect retained sweep and triangle counts; no volume query was omitted.".into()))?;
    if reserved > winding_terms {
        return Err(Error::Input(format!("Required boundary nesting and retained-sweep witnesses need {reserved} solid-angle terms, exceeding max_winding_terms={winding_terms}. Increase this explicit computation budget before reassessing; no sweep was skipped.")));
    }
    let inverse = pose.inverse();
    let mut by_source = std::collections::BTreeMap::new();
    for (i, region) in regions.iter().enumerate() {
        for o in &region.no_contact {
            let entry = by_source
                .entry((o.source.sweep.capture.as_str(), o.source.sweep.sequence))
                .or_insert_with(|| (Vec::new(), false));
            entry.0.push(i);
            entry.1 |= o.separation < 0.;
        }
    }
    let mut sweeps = Vec::new();
    for source in sources {
        let (hits, strict) = by_source
            .remove(&(source.sweep.capture.as_str(), source.sweep.sequence))
            .unwrap_or_default();
        let relation = if strict {
            Relation::BoundaryIntersection
        } else if !hits.is_empty() {
            Relation::BoundaryTouch
        } else {
            let model_point = inverse.point(source.sweep.center_from);
            if !finite(model_point) {
                return Err(Error::Data("A retained clear-sweep witness overflows the inverse candidate transform. Inspect coordinate units and the original pose; no inside/outside result was supplied.".into()));
            }
            let distance = solid.distance(model_point)?;
            if distance.inward == 0. {
                return Err(Error::Data("A clear-sweep witness lies on the required boundary although the full surface comparison reported no contact. Preserve the source analysis and inspect its transform/coordinate scale; no inconsistent volume relation was accepted.".into()));
            }
            if distance.inward > 0. {
                Relation::Inside {
                    model_point,
                    distance,
                }
            } else {
                Relation::Outside {
                    model_point,
                    distance,
                }
            }
        };
        sweeps.push(Sweep {
            source: source.clone(),
            regions: hits,
            relation,
        });
    }
    Ok(Some(Assessment {
        shells: solid.shells.clone(),
        topology_visits: solid.topology_visits,
        triangle_pairs: solid.triangle_pairs,
        vertices: solid.vertices,
        structure_winding_terms: solid.structure_winding_terms,
        reserved_winding_terms: reserved,
        sweeps,
    }))
}
