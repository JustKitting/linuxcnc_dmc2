use super::super::super::{geometry::*, probe::Sample, Error};
use super::{
    extract::{Mesh, Topology},
    field::Node,
    grid::Grid,
    request::Request,
};
use crate::object_map::record::quote;
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
pub enum Outcome {
    Empty,
    Open,
    EdgeClosed,
    Conflicting,
}
impl Outcome {
    pub fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Empty=>("stock-mesh-support-missing","No supported surface facets were reconstructed. Inspect lattice reasons, independent checks and calculation bounds; resolve source support or explicit interpolation settings and use a new analysis ID."),
            Self::Open=>("stock-mesh-open-proposal","The estimated mesh has open boundary edges. Inspect calculation cropping, missing measurements and facet support. Retain the open surface; do not fill its gaps by assuming a solid."),
            Self::EdgeClosed=>("stock-mesh-edge-closed-proposal","The estimated mesh has closed, consistently oriented edge incidence. Inspect vertex topology, unmeasured regions and geometric validity before treating it as a volume; edge counts do not establish physical material."),
            Self::Conflicting=>("stock-mesh-topology-conflict","The estimated mesh has nonmanifold or inconsistently oriented edges. Inspect its source field, fits and grid resolution before reuse; no topology repair or facet deletion was applied."),
        }
    }
}
pub struct Report {
    pub json: String,
    pub files: Vec<(&'static str, String)>,
    pub outcome: Outcome,
}
fn outcome(t: &Topology) -> Outcome {
    if t.triangles == 0 {
        Outcome::Empty
    } else if t.nonmanifold_edges > 0 || t.orientation_conflicts > 0 {
        Outcome::Conflicting
    } else if t.boundary_edges > 0 {
        Outcome::Open
    } else {
        Outcome::EdgeClosed
    }
}
pub fn build(
    grid: &Grid,
    nodes: &[Node],
    mesh: &Mesh,
    samples: &[Sample],
    r: &Request,
) -> Result<Report, Error> {
    let mut csv_rows=String::from("grid_vertex,machine_x_mm,machine_y_mm,machine_z_mm,state,signed_distance_mm,nearest_capture,nearest_sequence\n");
    let mut counts = BTreeMap::new();
    let mut recovery = BTreeMap::new();
    let mut known = 0usize;
    for (i, n) in nodes.iter().enumerate() {
        let s = &samples[n.seed];
        let (state, value) = match n.value {
            Ok(d) => {
                known += 1;
                ("estimated-local-distance", d.to_string())
            }
            Err(reason) => {
                let (state, message) = reason.description();
                *counts.entry(state).or_insert(0usize) += 1;
                recovery.insert(state, message);
                (state, String::new())
            }
        };
        csv_rows.push_str(&format!(
            "{i},{},{state},{value},{},{}\n",
            csv(grid.point(i)),
            s.capture.as_str(),
            s.sequence
        ));
    }
    let mut vertices=String::from("mesh_vertex,machine_x_mm,machine_y_mm,machine_z_mm,grid_endpoint_a,grid_endpoint_b,fraction_from_a\n");
    for (i, v) in mesh.vertices.iter().enumerate() {
        vertices.push_str(&format!(
            "{i},{},{},{},{}\n",
            csv(v.p),
            v.a,
            v.b,
            v.fraction
        ));
    }
    let mut facets=String::from("facet,mesh_vertex_a,mesh_vertex_b,mesh_vertex_c,grid_cell_origin,tetrahedron,state,support_capture,support_sequence\n");
    for (i, f) in mesh.facets.iter().enumerate() {
        let support = f
            .source
            .map(|seed| {
                format!(
                    "supported-local-interpolation,{},{}",
                    samples[seed].capture.as_str(),
                    samples[seed].sequence
                )
            })
            .unwrap_or_else(|| "full-facet-support-unresolved,,".into());
        facets.push_str(&format!(
            "{i},{},{},{},{},{},{support}\n",
            f.vertices[0], f.vertices[1], f.vertices[2], f.cell, f.tetrahedron
        ));
    }
    let mut cells = String::from(
        "grid_cell_origin,min_x_mm,min_y_mm,min_z_mm,max_x_mm,max_y_mm,max_z_mm,state\n",
    );
    let stride = 1 + grid.axes[0].len() + grid.axes[0].len() * grid.axes[1].len();
    for cell in &mesh.unresolved_cells {
        cells.push_str(&format!(
            "{cell},{},{},field-support-unresolved\n",
            csv(grid.point(*cell)),
            csv(grid.point(cell + stride))
        ));
    }
    let topology = mesh.topology();
    let outcome = outcome(&topology);
    let (state, message) = outcome.description();
    let unsupported = mesh.facets.iter().filter(|f| f.source.is_none()).count();
    let counts = counts
        .iter()
        .map(|(k, v)| format!("{}:{v}", quote(k)))
        .collect::<Vec<_>>()
        .join(",");
    let recovery = recovery
        .iter()
        .map(|(k, v)| format!("{}:{}", quote(k), quote(v)))
        .collect::<Vec<_>>()
        .join(",");
    let needs=format!("{{\"schema\":\"dmc2.stock-mesh-measurement-needs.v1\",\"surface_analysis\":{},\"frame\":\"LinuxCNC machine-mm\",\"unresolved_lattice_vertices\":{},\"lattice_reason_counts\":{{{counts}}},\"lattice_reason_recovery\":{{{recovery}}},\"lattice_source_table\":\"residuals.csv\",\"unresolved_cells\":{},\"cell_region_table\":\"unresolved-cells.csv\",\"unsupported_facets\":{unsupported},\"facet_support_table\":\"mesh-facets.csv\",\"mesh_vertex_table\":\"mesh-vertices.csv\",\"message\":\"Cell bounds and facet vertex references identify regions requiring support or interpolation review. They are calculation regions, not travel envelopes or probe endpoints. Independent physical acceptance and closed-volume modeling remain outstanding.\",\"machine_action_authorized\":false}}\n",quote(r.surface.as_str()),nodes.len()-known,mesh.unresolved_cells.len());
    let extent = grid.axes.each_ref().map(|a| a.len());
    let spacing = std::array::from_fn(|i| (r.max[i] - r.min[i]) / (extent[i] - 1) as f64);
    if !finite(spacing) {
        return Err(Error::Data(
            "Actual grid spacing is not finite. Inspect its numerical bounds before reuse.".into(),
        ));
    }
    let json=format!("{{\"schema\":\"dmc2.stock-mesh.v1\",\"state\":{},\"message\":{},\"frame\":\"LinuxCNC machine-mm\",\"bounds_min_mm\":{},\"bounds_max_mm\":{},\"grid_axis_vertices\":[{},{},{}],\"actual_grid_spacing_mm\":{},\"lattice_vertices\":{},\"supported_distance_vertices\":{known},\"mesh_vertices\":{},\"candidate_facets\":{},\"supported_facets\":{},\"unsupported_facets\":{unsupported},\"collapsed_candidate_facets\":{},\"boundary_edges\":{},\"nonmanifold_edges\":{},\"orientation_conflicts\":{},\"surface_file\":{},\"measurement_needs\":{},\"vertex_manifold_checked\":false,\"self_intersections_checked\":false,\"solid_stock\":null,\"unmeasured_volume\":\"unknown\",\"cam_ready\":false,\"interpretation\":\"An explicitly bounded lattice samples signed distances to the nearest original fitting station's eligible local plane. Source hull/gap support, the normal band and independent checks limit its domain; unresolved nearest patches do not fall back to farther planes. A conforming tetrahedral subdivision linearly interpolates sign crossings. Full facets require a common checked patch agreeing with their orientation and covering their projection and the requested interpolation residual. Unsupported candidate facets and missing-field cells remain in source-linked tables. The STL contains supported interpolation facets only and its open boundaries are preserved. This is an estimated surface, not a nominal stock shape or evidence of material occupancy. Sampling can miss sub-grid features; coordinate interpolation and edge incidence do not prove physical coverage, vertex manifoldness or absence of cavities.\"}}\n",quote(state),quote(message),json(r.min),json(r.max),extent[0],extent[1],extent[2],json(spacing),nodes.len(),mesh.vertices.len(),mesh.facets.len(),topology.triangles,mesh.collapsed_facets,topology.boundary_edges,topology.nonmanifold_edges,topology.orientation_conflicts,if topology.triangles>0 {quote("stock-surface.machine-mm.stl")}else{"null".into()},needs);
    let mut files = vec![
        ("residuals.csv", csv_rows),
        ("mesh-vertices.csv", vertices),
        ("mesh-facets.csv", facets),
        ("unresolved-cells.csv", cells),
        ("measurement-needs.json", needs),
    ];
    if topology.triangles > 0 {
        files.push(("stock-surface.machine-mm.stl", mesh.stl()));
    }
    Ok(Report {
        json,
        files,
        outcome,
    })
}
