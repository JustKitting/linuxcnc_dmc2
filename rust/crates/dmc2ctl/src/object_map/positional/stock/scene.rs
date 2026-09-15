//! Bind retained material/stock results for CAD inspection in one declared frame.
use super::super::{folder, mesh::Mesh, read_pose, retained::Bundle};
use super::{material, placement, reconstruction, surface};
use crate::object_map::{
    model::Id,
    record::{self, quote},
    store::{save, Store},
    Error,
};
use std::path::Path;

enum GeometryRole {
    RequiredMaterial,
    MeasuredSurface,
}
impl GeometryRole {
    fn description(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::RequiredMaterial=>("operation-retained-material","material/candidate-source-model-candidate.machine-mm.stl","Unchanged material the operation must preserve, including backing and holding geometry, at its unreviewed candidate placement."),
            Self::MeasuredSurface=>("estimated-measured-stock-surface","stock/stock-surface.machine-mm.stl","Supported measured interpolation facets. Open regions and unmeasured volume remain unresolved; this is not a CAM stock solid."),
        }
    }
    fn json(self) -> String {
        let (role, file, meaning) = self.description();
        format!("{{\"role\":{},\"file\":{},\"interpretation\":{},\"frame\":\"LinuxCNC machine-mm\",\"mm_per_unit\":1,\"import_placement\":\"identity\",\"additional_model_transform\":\"none\"}}",quote(role),quote(file),quote(meaning))
    }
}
pub fn export(
    store: &Store,
    object: &Id,
    setup: &Id,
    material_id: &Id,
    mesh_id: &Id,
    output: &Path,
) -> Result<String, Error> {
    if output.exists() {
        return Err(Error::Storage("This stock-scene directory already exists. Choose a new output directory; the previous handoff remains intact.".into()));
    }
    let object_label = store.object_label(object)?;
    let setup_label = store.setup_label(object, setup)?;
    let material = Bundle::read(&folder(store, object, setup, material_id)?)?;
    let mr = material::request::Request::read(material.get("request.txt")?)?;
    let stock = Bundle::read(&folder(store, object, setup, mesh_id)?)?;
    let reconstructed = reconstruction::estimate(store, object, setup, stock.get("request.txt")?)?;
    if mr.surface != reconstructed.request.surface {
        return Err(Error::Data("The selected material check and stock mesh use different surface analysis IDs. Select results from the same retained surface revision; no cross-source alignment was inferred.".into()));
    }
    // Both dependent records must carry exactly the same complete source bytes.
    material.require_source(&reconstructed.source.source, "surface-source-")?;
    stock.require_source(&reconstructed.source.source, "surface-source-")?;
    for (name, bytes) in &reconstructed.report.files {
        stock.require_equal(name, bytes.as_bytes())?;
    }
    stock.require_equal(
        "manifest.json",
        reconstruction::manifest(
            object,
            setup,
            mesh_id,
            &reconstructed.request,
            &reconstructed.report,
        )
        .as_bytes(),
    )?;
    let candidate_path = folder(store, object, setup, &mr.candidate)?;
    let candidate = Bundle::read(&candidate_path)?;
    material.require_source(&candidate, "candidate-source-")?;
    let pr = placement::request::Request::read(candidate.get("request.txt")?)?;
    let pose = read_pose(&candidate_path.join("pose-candidate.txt"))?;
    let required = Mesh::read(candidate.get("source.stl")?, pr.units)?;
    required.fitting_geometry()?;
    candidate.require_equal(
        "model-candidate.machine-mm.stl",
        required.transformed_stl(pose)?.as_bytes(),
    )?;
    let (outline_fields, _) = record::decode(
        candidate.get("stock-source-request.txt")?,
        super::request::SCHEMA,
        &super::request::keys(),
    )?;
    let (surface_fields, _) = record::decode(
        reconstructed.source.source.get("request.txt")?,
        surface::request::SCHEMA,
        &surface::request::keys(),
    )?;
    if outline_fields["frame_reference"] != surface_fields["frame_reference"] {
        return Err(Error::Data("The candidate outline and stock surface declare different reference frames. Resolve their setup relationship and recalculate matching analyses before exporting this scene.".into()));
    }
    let has_surface = reconstructed
        .report
        .files
        .iter()
        .any(|(name, _)| *name == "stock-surface.machine-mm.stl");
    let mut geometry = vec![GeometryRole::RequiredMaterial.json()];
    if has_surface {
        geometry.push(GeometryRole::MeasuredSurface.json());
    }
    let (stock_state, stock_message) = reconstructed.report.outcome.description();
    let scene=format!("{{\"schema\":\"dmc2.freecad-stock-scene.v1\",\"object\":{},\"object_label\":{},\"setup\":{},\"setup_label\":{},\"state\":\"unreviewed-stock-scene\",\"frame_reference\":{},\"frame_relationship_evidence\":\"Matching declared source references and exact retained source bytes; physical registration remains unaccepted.\",\"geometry\":[{}],\"required_design_revision\":{},\"original_design_stl\":\"material/candidate-source-source.stl\",\"original_design_mm_per_unit\":{},\"candidate_model_mm_to_machine_mm\":{},\"transform_convention\":\"Row-major matrix multiplying homogeneous column vectors. Convert original design coordinates to model millimetres before applying this candidate. Scene geometry is already in machine millimetres; import it at identity without applying this matrix or a setup flip again.\",\"candidate_pose_record\":\"material/candidate-source-pose-candidate.txt\",\"candidate_analysis\":{},\"material_analysis\":{},\"stock_mesh_analysis\":{},\"surface_analysis\":{},\"material_report\":\"material/material-check.machine-mm.json\",\"material_measurement_needs\":\"material/measurement-needs.json\",\"stock_report\":\"stock/manifest.json\",\"stock_measurement_needs\":\"stock/measurement-needs.json\",\"stock_state\":{},\"stock_message\":{},\"stock_reconstruction_replayed\":true,\"material_calculation_replayed\":false,\"material_report_interpretation\":\"Retained assessment with exact candidate/surface source binding; export does not rerun its local material comparison or accept containment.\",\"solid_stock\":null,\"placement_accepted\":false,\"native_cam_job\":null,\"cam_ready\":false,\"machine_action_authorized\":false}}\n",quote(object.as_str()),quote(&object_label),quote(setup.as_str()),quote(&setup_label),quote(&surface_fields["frame_reference"]),geometry.join(","),quote(pr.design.as_str()),pr.units,pose.json(),quote(mr.candidate.as_str()),quote(material_id.as_str()),quote(mesh_id.as_str()),quote(mr.surface.as_str()),quote(stock_state),quote(stock_message));
    // Compute/read all required material outputs before publishing a scene.
    material.get("material-check.machine-mm.json")?;
    material.get("measurement-needs.json")?;
    material.copy_to(&output.join("material"), "")?;
    stock.copy_to(&output.join("stock"), "")?;
    save(&output.join("README.txt"),b"DMC2 measured stock / required material scene\n\nOpen the files listed in manifest.json geometry as separate meshes in FreeCAD. Those files use LinuxCNC machine millimetres and identity import placement. The required material mesh already includes its candidate transform. Do not apply that transform, a work offset or a CAD setup flip again.\n\nThe original design STL and its unit conversion and candidate matrix are retained separately for native CAD/CAM integration. Required operation material and estimated measured stock have distinct roles. For an operation that retains backing or holding features, its required mesh must include them.\n\nInspect the material and stock reports and their measurement needs. An open or unsupported stock surface stays open or absent. The scene does not establish a stock solid, accepted placement, native CAM Job, tools, fixtures or a cutting program. Retained original records are in the source subdirectories.\n\nmanifest.json is published last. A directory without it is an interrupted export: preserve it and retry with a new output directory.\n")?;
    save(&output.join("manifest.json"), scene.as_bytes())?;
    Ok(format!("{{\"export\":{},\"state\":\"unreviewed-stock-scene\",\"message\":\"The stock surface and required material are exported in their declared common machine frame, with original sources and placement metadata. Inspect the scene and measurement needs; native CAM and physical placement remain unresolved.\",\"cam_ready\":false}}",quote(&output.display().to_string())))
}
