//! Immutable triangle IDs in a balanced axis-aligned bounding-box hierarchy.
use super::{Error, Nearest, Triangle, V, data, finite};

#[derive(Clone, Copy)]
struct Bounds {
    min: V,
    max: V,
}

impl Bounds {
    fn overlaps(self, other: Self) -> bool {
        (0..3).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }
    fn triangle(t: &Triangle) -> Self {
        Self {
            min: std::array::from_fn(|i| t.v.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)),
            max: std::array::from_fn(|i| {
                t.v.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max)
            }),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }

    fn center(self, axis: usize) -> f64 {
        self.min[axis] / 2. + self.max[axis] / 2.
    }

    fn distance_lower_bound(self, p: V) -> f64 {
        // The largest coordinate gap is a lower bound on Euclidean distance.
        // Round each subtraction down to the adjacent representable value;
        // there is no dimensional epsilon or machining tolerance here.
        (0..3)
            .map(|i| {
                let gap = (self.min[i] - p[i]).max(p[i] - self.max[i]).max(0.);
                if gap > 0. {
                    f64::from_bits(gap.to_bits() - 1)
                } else {
                    0.
                }
            })
            .fold(0., f64::max)
    }
}

enum Contents {
    Triangle(usize),
    Branch([usize; 2]),
}

struct Node {
    bounds: Bounds,
    contents: Contents,
}

pub(super) struct Index {
    nodes: Vec<Node>,
    root: usize,
}

impl Index {
    /// Visit every unordered pair of intersecting closed triangle AABBs.
    /// The explicit budget counts hierarchy visits, including pruned nodes.
    pub(super) fn overlap_pairs(
        &self,
        triangles: &[Triangle],
        budget: usize,
        mut pair: impl FnMut(usize, usize) -> Result<(), Error>,
    ) -> Result<usize, Error> {
        let mut visits = 0usize;
        for (i, t) in triangles.iter().enumerate() {
            let bounds = Bounds::triangle(t);
            let mut pending = vec![self.root];
            while let Some(id) = pending.pop() {
                if visits == budget {
                    return Err(data(
                        "The mesh topology search exhausted max_topology_visits. Preserve the source and increase this computational budget in a new request; no closed-solid result was accepted.",
                    ));
                }
                visits += 1;
                let node = &self.nodes[id];
                if !bounds.overlaps(node.bounds) {
                    continue;
                }
                match node.contents {
                    Contents::Triangle(j) if j > i => pair(i, j)?,
                    Contents::Triangle(_) => (),
                    Contents::Branch(children) => pending.extend(children),
                }
            }
        }
        Ok(visits)
    }
    pub(super) fn new(triangles: &[Triangle]) -> Result<Self, Error> {
        if triangles.is_empty() {
            return Err(data("STL has no searchable geometry."));
        }
        let mut items = triangles
            .iter()
            .enumerate()
            .map(|(id, t)| (id, Bounds::triangle(t)))
            .collect::<Vec<_>>();
        let mut nodes = Vec::new();
        let root = Self::build(&mut nodes, &mut items);
        Ok(Self { nodes, root })
    }

    fn build(nodes: &mut Vec<Node>, items: &mut [(usize, Bounds)]) -> usize {
        let bounds = items.iter().fold(items[0].1, |b, (_, next)| b.union(*next));
        let contents = if items.len() == 1 {
            Contents::Triangle(items[0].0)
        } else {
            let axis = (0..3)
                .max_by(|a, b| {
                    (bounds.max[*a] - bounds.min[*a]).total_cmp(&(bounds.max[*b] - bounds.min[*b]))
                })
                .unwrap();
            let middle = items.len() / 2;
            items.select_nth_unstable_by(middle, |a, b| {
                a.1.center(axis)
                    .total_cmp(&b.1.center(axis))
                    .then(a.0.cmp(&b.0))
            });
            let (left, right) = items.split_at_mut(middle);
            Contents::Branch([Self::build(nodes, left), Self::build(nodes, right)])
        };
        let id = nodes.len();
        nodes.push(Node { bounds, contents });
        id
    }

    pub(super) fn nearest(&self, triangles: &[Triangle], p: V) -> Result<Nearest, Error> {
        if !finite(p) {
            return Err(data(
                "Closest-triangle query is nonfinite; check STL units and initial placement.",
            ));
        }
        let mut best = None;
        self.visit(self.root, triangles, p, &mut best)?;
        best.ok_or_else(|| data("STL has no searchable geometry."))
    }

    fn visit(
        &self,
        id: usize,
        triangles: &[Triangle],
        p: V,
        best: &mut Option<Nearest>,
    ) -> Result<(), Error> {
        let node = &self.nodes[id];
        if let Some(current) = best {
            // Enlarge the nonnegative radius by one representable value. Keep
            // equality searchable so traversal order cannot change a tie.
            let radius = f64::from_bits(current.distance.abs().to_bits() + 1);
            if node.bounds.distance_lower_bound(p) > radius {
                return Ok(());
            }
        }
        match node.contents {
            Contents::Triangle(triangle) => {
                let candidate = triangles[triangle].closest(p, triangle)?;
                if best.as_ref().is_none_or(|current| {
                    candidate.distance.abs() < current.distance.abs()
                        || (candidate.distance.abs() == current.distance.abs()
                            && candidate.triangle < current.triangle)
                }) {
                    *best = Some(candidate);
                }
            }
            Contents::Branch([left, right]) => {
                let left_gap = self.nodes[left].bounds.distance_lower_bound(p);
                let right_gap = self.nodes[right].bounds.distance_lower_bound(p);
                let order = if left_gap <= right_gap {
                    [left, right]
                } else {
                    [right, left]
                };
                for next in order {
                    self.visit(next, triangles, p, best)?;
                }
            }
        }
        Ok(())
    }
}
