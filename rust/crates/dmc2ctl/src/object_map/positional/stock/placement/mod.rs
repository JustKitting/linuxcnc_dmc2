//! Connect a retained measured outline to unchanged machining geometry.
use super::super::cover;
mod polygon;
mod report;
pub(in crate::object_map) mod request;
mod search;
#[cfg(test)]
mod tests;
use super::super::{mesh::Mesh, request::Use, retained::Bundle};
use super::{
    fit,
    request::{Closure, Surface},
};
use crate::object_map::{
    model::{DesignFormat, Id},
    record,
    store::{read, save, Store},
    Error,
};
use std::path::Path;
struct Source {
    polygon: polygon::Polygon,
    report: String,
    files: Bundle,
}
fn source(store: &Store, object: &Id, setup: &Id, id: &Id) -> Result<Source, Error> {
    let path = super::super::folder(store, object, setup, id)?;
    let files = Bundle::read(&path)?;
    let req = super::request::Request::read(files.get("request.txt")?)?;
    if req.closure != Closure::Closed || req.surface != Surface::VerticalSides {
        return Err(Error::Input("Footprint placement needs a closed source outline with explicit vertical-sides ball correction. Select or prepare that analysis; this assumption still does not establish a stock volume.".into()));
    }
    let captures = store.captures(object, setup)?;
    let samples = req.probe.samples(&captures, &req.selected)?;
    files.check_captures(&captures, &samples)?;
    let contour = fit::run(&samples, &req)?;
    let report = super::report::build(&samples, &contour, &req)?.json;
    files.require_equal("stock-outline.machine-mm.json", report.as_bytes())?;
    let stations = &contour.stations;
    for (i, a) in stations.iter().enumerate() {
        let b = &stations[(i + 1) % stations.len()];
        if a.stop != fit::Stop::Converged
            || a.residuals.iter().any(|x| x.abs() > req.max_residual)
            || fit::distance(samples[a.sample].center, samples[b.sample].center) > req.max_gap
        {
            return Err(Error::Data(format!("Outline station/interval {i} has unresolved fit or sampling support. Resolve the source analysis's measurement requirements before fitting a footprint.")));
        }
    }
    let (min, max) = stations
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.center[2]), b.max(p.center[2]))
        });
    if max - min > req.max_z_span {
        return Err(Error::Data("The source outline combines incompatible heights. Resolve its height slices before footprint placement.".into()));
    }
    let checks = samples
        .iter()
        .filter(|s| s.usage == Use::Check)
        .collect::<Vec<_>>();
    if checks.is_empty() {
        return Err(Error::Data("The source outline has no independent check contacts. Retain and review independent checks before using it for a footprint.".into()));
    }
    for check in checks {
        let residual = stations
            .iter()
            .enumerate()
            .map(|(i, a)| {
                super::report::projection(
                    check.center,
                    a.center,
                    stations[(i + 1) % stations.len()].center,
                )
                .1
            })
            .fold(f64::INFINITY, f64::min);
        if residual > req.max_residual {
            return Err(Error::Data(format!("Independent outline check {}:{} disagrees by {residual} mm. Resolve the source estimate; the check will not become fitting data.",check.capture.as_str(),check.sequence)));
        }
    }
    let polygon = polygon::Polygon::new(stations.iter().map(|p| p.surface.unwrap()).collect())?;
    Ok(Source {
        polygon,
        report,
        files,
    })
}
pub fn prepare(
    store: &Store,
    object: &Id,
    setup: &Id,
    outline: &Id,
    design: &Id,
) -> Result<String, Error> {
    let path = super::super::folder(store, object, setup, outline)?;
    read(&path.join("manifest.json"))?;
    super::request::Request::read(&read(&path.join("request.txt"))?)?;
    if !store
        .designs(object)?
        .iter()
        .any(|d| d.id == *design && d.format == DesignFormat::Stl)
    {
        return Err(Error::Input("Attach the complete required operation geometry as an STL revision, then select it for the footprint.".into()));
    }
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                match *k {
                    "outline_analysis" => outline.as_str(),
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
    let output = super::super::folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage(
            "The footprint analysis ID already exists; use a new ID to preserve its result.".into(),
        ));
    }
    let raw = read(input)?;
    let r = request::Request::read(&raw)?;
    let source = source(store, object, setup, &r.outline)?;
    let designs = store.designs(object)?;
    let design=designs.iter().find(|d|d.id==r.design && d.format==DesignFormat::Stl).ok_or_else(||Error::Input("The required operation STL is not attached to this object. Attach that revision and correct the request.".into()))?;
    let mesh = Mesh::read(&design.raw, r.units)?;
    mesh.fitting_geometry()?;
    let samples = cover::build(
        mesh.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Horizontal,
    )?;
    let fitted = search::run(&source.polygon, &samples, &r)?;
    let result = report::build(&source.polygon, &samples, &fitted, &r)?;
    let pose = search::pose(fitted.at, r.z).validate()?;
    let transformed = mesh.transformed_stl(pose)?;
    // Compute before publishing. Source bundles and original STL bytes survive
    // alongside every candidate, including unsuccessful numerical searches.
    save(&output.join("request.txt"), &raw)?;
    save(&output.join("source.stl"), &design.raw)?;
    source.files.copy_to(&output, "stock-source-")?;
    save(
        &output.join("model-candidate.machine-mm.stl"),
        transformed.as_bytes(),
    )?;
    save(&output.join("residuals.csv"), result.csv.as_bytes())?;
    save(
        &output.join("search-history.csv"),
        result.history.as_bytes(),
    )?;
    if fitted.stop == search::Stop::Clearance {
        save(
            &output.join("pose-candidate.txt"),
            &super::super::pose_record(pose)?,
        )?;
    }
    let (state, message) = fitted.stop.description();
    let manifest=format!("{{\"schema\":\"dmc2.footprint-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"design\":{},\"outline_analysis\":{},\"result\":{},\"source_outline\":{},\"cam_ready\":false}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()),record::quote(r.design.as_str()),record::quote(r.outline.as_str()),result.json,source.report);
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":{},\"message\":{},\"cam_ready\":false}}",
        record::quote(&output.display().to_string()),
        record::quote(state),
        record::quote(message)
    ))
}
