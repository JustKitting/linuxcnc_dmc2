//! Piecewise-linear isosurface extraction with shared indexed intersections.
use super::super::super::{geometry::*, Error};
use super::{field::Node, grid::Grid};
use std::collections::{BTreeMap, BTreeSet};
// Binary XYZ corner indices, six tetrahedra sharing the 0--7 diagonal.
const TETRAHEDRA: [[usize; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 3, 2, 7],
    [0, 2, 6, 7],
    [0, 6, 4, 7],
    [0, 4, 5, 7],
    [0, 5, 1, 7],
];
pub struct Vertex {
    pub p: V,
    pub a: usize,
    pub b: usize,
    pub fraction: f64,
}
pub struct Facet {
    pub vertices: [usize; 3],
    pub cell: usize,
    pub tetrahedron: usize,
    pub source: Option<usize>,
}
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub facets: Vec<Facet>,
    pub unresolved_cells: BTreeSet<usize>,
    pub collapsed_facets: usize,
    intersections: BTreeMap<(usize, usize), usize>,
}
impl Mesh {
    fn vertex(&mut self, a: usize, b: usize, grid: &Grid, nodes: &[Node]) -> Result<usize, Error> {
        let (mut a, mut b) = (a.min(b), a.max(b));
        let da = nodes[a].value.unwrap();
        let db = nodes[b].value.unwrap();
        let fraction = if da == 0. {
            b = a;
            0.
        } else if db == 0. {
            a = b;
            0.
        } else {
            let scale = da.abs().max(db.abs());
            let da = da.abs() / scale;
            let db = db.abs() / scale;
            da / (da + db)
        };
        if let Some(index) = self.intersections.get(&(a, b)) {
            return Ok(*index);
        }
        let p = add(
            grid.point(a),
            scale(sub(grid.point(b), grid.point(a)), fraction),
        );
        if !finite(p) || !fraction.is_finite() {
            return Err(Error::Data("A reconstructed edge intersection is not finite. Inspect grid units and source distances.".into()));
        }
        let index = self.vertices.len();
        self.vertices.push(Vertex { p, a, b, fraction });
        self.intersections.insert((a, b), index);
        Ok(index)
    }
    fn facet(
        &mut self,
        mut vertices: [usize; 3],
        direction: V,
        cell: usize,
        tetrahedron: usize,
        budget: usize,
        support: &impl Fn(&[V; 3]) -> Option<usize>,
    ) -> Result<(), Error> {
        let p = vertices.map(|i| self.vertices[i].p);
        let normal = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let length = norm(normal);
        if length == 0. {
            self.collapsed_facets += 1;
            return Ok(());
        }
        if !length.is_finite() {
            return Err(Error::Data(
                "Reconstructed facet normal overflowed. Inspect grid coordinate scales.".into(),
            ));
        }
        let facing = dot(normal.map(|v| v / length), direction);
        if !facing.is_finite() || facing == 0. {
            return Err(Error::Data("A reconstructed facet cannot be oriented at these coordinate scales. Inspect its grid spacing and units; no arbitrary normal was assigned.".into()));
        }
        if facing < 0. {
            vertices.swap(1, 2);
        }
        if self.facets.len() >= budget {
            return Err(Error::Input("max_mesh_triangles was exhausted during reconstruction. Increase the explicit budget or coarsen/narrow the grid; no partial result was published as the requested mesh.".into()));
        }
        self.facets.push(Facet {
            vertices,
            cell,
            tetrahedron,
            source: support(&vertices.map(|i| self.vertices[i].p)),
        });
        Ok(())
    }
    pub fn topology(&self) -> Topology {
        let mut edges: BTreeMap<(usize, usize), (usize, isize)> = BTreeMap::new();
        let mut triangles = 0;
        for f in self.facets.iter().filter(|f| f.source.is_some()) {
            triangles += 1;
            for j in 0..3 {
                let (a, b) = (f.vertices[j], f.vertices[(j + 1) % 3]);
                let e = edges.entry((a.min(b), a.max(b))).or_default();
                e.0 += 1;
                e.1 += if a < b { 1 } else { -1 };
            }
        }
        Topology {
            triangles,
            boundary_edges: edges.values().filter(|v| v.0 == 1).count(),
            nonmanifold_edges: edges.values().filter(|v| v.0 > 2).count(),
            orientation_conflicts: edges.values().filter(|v| v.0 == 2 && v.1 != 0).count(),
        }
    }
    pub fn stl(&self) -> String {
        let mut out = String::from("solid dmc2_estimated_stock_surface\n");
        for f in self.facets.iter().filter(|f| f.source.is_some()) {
            let p = f.vertices.map(|i| self.vertices[i].p);
            let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            let n = n.map(|v| v / norm(n));
            out.push_str(&format!(
                "facet normal {}\nouter loop\n",
                csv(n).replace(',', " ")
            ));
            for p in p {
                out.push_str(&format!("vertex {}\n", csv(p).replace(',', " ")));
            }
            out.push_str("endloop\nendfacet\n");
        }
        out.push_str("endsolid dmc2_estimated_stock_surface\n");
        out
    }
}
pub struct Topology {
    pub triangles: usize,
    pub boundary_edges: usize,
    pub nonmanifold_edges: usize,
    pub orientation_conflicts: usize,
}
pub fn run(
    grid: &Grid,
    nodes: &[Node],
    budget: usize,
    support: impl Fn(&[V; 3]) -> Option<usize>,
) -> Result<Mesh, Error> {
    if nodes.len() != grid.count {
        return Err(Error::Data("Reconstruction field and lattice sizes disagree. Recalculate from the retained request; no mesh was published.".into()));
    }
    let mut mesh = Mesh {
        vertices: vec![],
        facets: vec![],
        unresolved_cells: BTreeSet::new(),
        collapsed_facets: 0,
        intersections: BTreeMap::new(),
    };
    for z in 0..grid.axes[2].len() - 1 {
        for y in 0..grid.axes[1].len() - 1 {
            for x in 0..grid.axes[0].len() - 1 {
                let cube = grid.cube([x, y, z]);
                let cell = cube[0];
                for (number, t) in TETRAHEDRA.iter().enumerate() {
                    let indices = t.map(|i| cube[i]);
                    if indices.iter().any(|i| nodes[*i].value.is_err()) {
                        mesh.unresolved_cells.insert(cell);
                        continue;
                    }
                    let (inside, outside): (Vec<_>, Vec<_>) = indices
                        .into_iter()
                        .partition(|i| nodes[*i].value.unwrap() < 0.);
                    if inside.is_empty() || outside.is_empty() {
                        continue;
                    }
                    let anchor = grid.point(indices[0]);
                    let mean = |set: &[usize]| {
                        set.iter()
                            .map(|i| sub(grid.point(*i), anchor))
                            .fold([0.; 3], |a, p| add(a, scale(p, 1. / set.len() as f64)))
                    };
                    let direction = sub(mean(&outside), mean(&inside));
                    if inside.len() == 2 {
                        let a = mesh.vertex(inside[0], outside[0], grid, nodes)?;
                        let b = mesh.vertex(inside[0], outside[1], grid, nodes)?;
                        let c = mesh.vertex(inside[1], outside[0], grid, nodes)?;
                        let d = mesh.vertex(inside[1], outside[1], grid, nodes)?;
                        mesh.facet([a, b, c], direction, cell, number, budget, &support)?;
                        mesh.facet([b, d, c], direction, cell, number, budget, &support)?;
                    } else {
                        let (one, many) = if inside.len() == 1 {
                            (&inside, &outside)
                        } else {
                            (&outside, &inside)
                        };
                        let vertices = [
                            mesh.vertex(one[0], many[0], grid, nodes)?,
                            mesh.vertex(one[0], many[1], grid, nodes)?,
                            mesh.vertex(one[0], many[2], grid, nodes)?,
                        ];
                        mesh.facet(vertices, direction, cell, number, budget, &support)?;
                    }
                }
            }
        }
    }
    Ok(mesh)
}
