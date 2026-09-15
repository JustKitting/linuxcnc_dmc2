//! A shared typed candidate reader for material checking and CAD scene export.
use super::super::{folder, geometry::Pose, mesh::Mesh, read_pose, retained::Bundle};
use super::{partial, placement, surface, volume};
use crate::object_map::{model::Id, record, store::Store, Error};
enum Source {
    Adaptive,
    Footprint,
    Partial,
    Enclosed { stock: Id },
}
pub(super) struct Candidate {
    pub bundle: Bundle,
    pub pose: Pose,
    pub mesh: Mesh,
    pub design: Id,
    pub units: f64,
    pub frame: String,
    source: Source,
}
impl Candidate {
    pub fn load(store: &Store, object: &Id, setup: &Id, id: &Id) -> Result<Self, Error> {
        let dir = folder(store, object, setup, id)?;
        let bundle = Bundle::read(&dir)?;
        let raw = bundle.get("request.txt")?;
        let schema = raw.split(|b| *b == b'\n').next().unwrap_or_default();
        let (source, design, units, frame) = if schema == placement::request::SCHEMA.as_bytes() {
            let r = placement::request::Request::read(raw)?;
            let (fields, _) = record::decode(
                bundle.get("stock-source-request.txt")?,
                super::request::SCHEMA,
                &super::request::keys(),
            )?;
            (
                Source::Footprint,
                r.design,
                r.units,
                fields["frame_reference"].clone(),
            )
        } else if schema == volume::request::SCHEMA.as_bytes() {
            let r = volume::request::Request::read(raw)?;
            let (fields, _) =
                surface::request::decode(bundle.get("stock-source-surface-source-request.txt")?)?;
            (
                Source::Enclosed { stock: r.stock },
                r.design,
                r.units,
                fields["frame_reference"].clone(),
            )
        } else if schema == partial::request::SCHEMA.as_bytes() {
            let r = partial::request::Request::read(raw)?;
            let (fields, _) = surface::request::decode(bundle.get("surface-source-request.txt")?)?;
            (
                Source::Partial,
                r.design,
                r.units,
                fields["frame_reference"].clone(),
            )
        } else if schema == super::adaptive::request::SCHEMA.as_bytes() {
            let r=super::adaptive::request::Request::read(raw)?;
            let (fields,_)=surface::request::decode(bundle.get("surface-source-request.txt")?)?;
            (Source::Adaptive,r.design,r.units,fields["frame_reference"].clone())
        } else {
            return Err(Error::Input("Select a machining footprint, partial-placement or volume-placement candidate. Registration fits and stock estimates have different roles; no candidate type was inferred.".into()));
        };
        let pose = read_pose(&dir.join("pose-candidate.txt"))?;
        let mesh = Mesh::read(bundle.get("source.stl")?, units)?;
        mesh.fitting_geometry()?;
        bundle.require_equal(
            "model-candidate.machine-mm.stl",
            mesh.transformed_stl(pose)?.as_bytes(),
        )?;
        Ok(Self {
            bundle,
            pose,
            mesh,
            design,
            units,
            frame,
            source,
        })
    }
    pub fn require_frame(&self, surface: &Bundle) -> Result<(), Error> {
        let (fields, _) = surface::request::decode(surface.get("request.txt")?)?;
        if self.frame != fields["frame_reference"] {
            return Err(Error::Data("The placement candidate and 3D surfaces declare different frame references. Resolve their actual setup relationship and retain matching analyses; no implicit registration was applied.".into()));
        }
        Ok(())
    }
    pub fn require_stock(&self, id: &Id, stock: &Bundle) -> Result<(), Error> {
        if let Source::Enclosed { stock: expected } = &self.source {
            if expected != id {
                return Err(Error::Data("The volume-placement candidate uses a different stock-mesh revision. Select its retained stock mesh or calculate a new placement; no stock revision was substituted.".into()));
            }
            self.bundle.require_source(stock, "stock-source-")?;
        }
        Ok(())
    }
}
