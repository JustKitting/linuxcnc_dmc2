//! Place unchanged operation geometry within explicitly enclosed measured stock.
mod report;
pub(in crate::object_map) mod request;
mod search;
#[cfg(test)]
mod tests;
use super::super::{
    cover, folder,
    mesh::{Mesh, solid::Solid},
    pose_record,
    retained::Bundle,
};
use super::reconstruction;
use crate::object_map::{
    Error,
    model::{DesignFormat, Id},
    record,
    store::{Store, read, save},
};
use std::path::Path;
pub fn prepare(
    store: &Store,
    object: &Id,
    setup: &Id,
    stock: &Id,
    design: &Id,
) -> Result<String, Error> {
    let source = Bundle::read(&folder(store, object, setup, stock)?)?;
    reconstruction::request::Request::read(source.get("request.txt")?)?;
    if !store
        .designs(object)?
        .iter()
        .any(|d| d.id == *design && d.format == DesignFormat::Stl)
    {
        return Err(Error::Input("Attach the full required operation geometry as an STL revision before preparing its volume placement.".into()));
    }
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                match *k {
                    "stock_mesh_analysis" => stock.as_str(),
                    "design" => design.as_str(),
                    _ => "REQUIRED",
                },
            )
        })
        .collect::<Vec<_>>();
    String::from_utf8(record::encode(request::SCHEMA, &fields, &[])?)
        .map_err(|e| Error::Data(e.to_string()))
}
pub fn run(store: &Store, object: &Id, setup: &Id, id: &Id, input: &Path) -> Result<String, Error> {
    let output = folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage("This volume-placement analysis ID exists. Select a new ID to preserve the prior result.".into()));
    }
    let raw = read(input)?;
    let r = request::Request::read(&raw)?;
    let source = reconstruction::load(store, object, setup, &r.stock)?;
    let stock_bytes=source.estimated.report.files.iter().find(|(name,_)| *name=="stock-surface.machine-mm.stl").ok_or_else(||Error::Data("The reproduced stock reconstruction contains no supported surface mesh. Inspect its measurement needs and resolve the source observations before volume placement; an extra file cannot supply missing geometry.".into()))?;
    let stock_mesh = Mesh::read(stock_bytes.1.as_bytes(), 1.)?;
    let stock = Solid::new(&stock_mesh, r.topology_visits)?;
    let designs = store.designs(object)?;
    let design=designs.iter().find(|d|d.id==r.design && d.format==DesignFormat::Stl).ok_or_else(||Error::Input("The required operation STL revision is not attached to this object. Attach it and correct the request.".into()))?;
    let mesh = Mesh::read(&design.raw, r.units)?;
    mesh.fitting_geometry()?;
    let samples = cover::build(
        mesh.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Spatial,
    )?;
    let fitted = search::run(&stock, &samples, &r)?;
    let result = report::build(&stock, &samples, &fitted, &r)?;
    let pose = search::pose(fitted.at).validate()?;
    let transformed = mesh.transformed_stl(pose)?;
    save(&output.join("request.txt"), &raw)?;
    save(&output.join("source.stl"), &design.raw)?;
    source.bundle.copy_to(&output, "stock-source-")?;
    save(
        &output.join("model-candidate.machine-mm.stl"),
        transformed.as_bytes(),
    )?;
    save(&output.join("residuals.csv"), result.csv.as_bytes())?;
    save(
        &output.join("search-history.csv"),
        result.history.as_bytes(),
    )?;
    // Every numerical result remains inspectable, including a deficit/budget
    // result. This shared candidate record is always explicitly unreviewed.
    save(&output.join("pose-candidate.txt"), &pose_record(pose)?)?;
    let manifest = format!(
        "{{\"schema\":\"dmc2.volume-placement-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"design\":{},\"stock_mesh_analysis\":{},\"surface_analysis\":{},\"result\":{},\"cam_ready\":false}}\n",
        record::quote(object.as_str()),
        record::quote(setup.as_str()),
        record::quote(id.as_str()),
        record::quote(r.design.as_str()),
        record::quote(r.stock.as_str()),
        record::quote(source.estimated.request.surface.as_str()),
        result.json
    );
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    let (state, message) = search::description(fitted.stop);
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":{},\"message\":{},\"cam_ready\":false}}",
        record::quote(&output.display().to_string()),
        record::quote(state),
        record::quote(message)
    ))
}
