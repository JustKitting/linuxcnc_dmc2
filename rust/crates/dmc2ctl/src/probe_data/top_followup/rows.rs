//! Typed new columns or source-qualified original top repeats.
use super::{Request, Settings, TopColumn};
use crate::probe_data::mapper_schema::Phase;
use std::collections::BTreeSet;

const COLUMNS: &str = "x_work_mm,y_work_mm";
const REPEATS: &str = "proposal,capture,sequence,phase,edge,approach_x_work_mm,approach_y_work_mm,target_x_work_mm,target_y_work_mm,target_z_work_mm";
const DIRECTED: &str = "phase,edge,approach_x_work_mm,approach_y_work_mm,target_x_work_mm,target_y_work_mm,target_z_work_mm";

pub struct Repeat {
    pub proposal: usize,
    pub capture: String,
    pub sequence: usize,
    pub request: Request,
}
pub enum Rows {
    Columns(Vec<[f64; 2]>),
    Repeats(Vec<Repeat>),
    Directed(Vec<Request>),
}
pub(super) fn identifier(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err(format!("Follow-up {name} is not a retained ID. Export the program from its original Object Mapper analysis."));
    }
    Ok(())
}
impl Rows {
    pub fn len(&self) -> usize {
        match self {
            Self::Columns(rows) => rows.len(),
            Self::Repeats(rows) => rows.len(),
            Self::Directed(rows) => rows.len(),
        }
    }
    pub fn repeated(&self) -> bool {
        matches!(self, Self::Repeats(_))
    }
    pub fn directed(&self) -> bool {
        matches!(self, Self::Directed(_))
    }
    pub(super) fn requests(&self, s: &Settings) -> Result<Vec<Request>, String> {
        if self.len() == 0 {
            return Err("The follow-up plan contains no selected top columns. Select an analysis with eligible observations; no empty program was supplied.".into());
        }
        match self {
            Self::Directed(rows) => rows.iter().map(|q| {
                let side=q.phase==Phase::Rim;
                let changed=(0..2).filter(|&i| !super::super::mapper_settings::close(q.approach[i],q.target[i])).count();
                if (side && (changed!=1 || !(0..=3).contains(&q.edge)))
                    || (!side && (q.phase!=Phase::Grid || changed!=0 || q.edge!=-1 || q.target[2]>=s.origin[2])) {
                    return Err("An adaptive observation must be a downward column or one horizontal side approach. Inspect its explicit start, target and direction; no replacement move was supplied.".into());
                }
                for p in [s.origin,[q.approach[0],q.approach[1],s.origin[2]],
                    [q.approach[0],q.approach[1],if side {q.target[2]} else {s.origin[2]}],q.target] {s.bounds(p)?;}
                Ok(*q)
            }).collect(),
            Self::Columns(rows) => rows
                .iter()
                .map(|&xy| TopColumn::new(s, xy).map(|p| p.request))
                .collect(),
            Self::Repeats(rows) => {
                let mut sources = BTreeSet::new();
                let mut proposals = BTreeSet::new();
                rows.iter().map(|r| {
                    identifier("repeat source capture", &r.capture)?;
                    if !sources.insert((&r.capture, r.sequence)) || !proposals.insert(r.proposal) {
                        return Err("A top repeat duplicates an original source record or proposal. Preserve the plan and re-export the selected observation analysis; no duplicate row was removed silently.".into());
                    }
                    TopColumn::from_request(s, r.request).map(|p| p.request)
                }).collect()
            }
        }
    }
    pub(super) fn encode(&self) -> String {
        match self {
            Self::Directed(rows) => {
                let mut out = format!("{DIRECTED}\n");
                for q in rows {
                    out.push_str(&format!(
                        "{},{},{},{},{},{},{}\n",
                        q.phase as u8,
                        q.edge,
                        q.approach[0],
                        q.approach[1],
                        q.target[0],
                        q.target[1],
                        q.target[2]
                    ));
                }
                out
            }
            Self::Columns(rows) => {
                let mut out = format!("{COLUMNS}\n");
                for xy in rows {
                    out.push_str(&format!("{},{}\n", xy[0], xy[1]));
                }
                out
            }
            Self::Repeats(rows) => {
                let mut out = format!("{REPEATS}\n");
                for r in rows {
                    let q = r.request;
                    out.push_str(&format!(
                        "{},{},{},{},{},{},{},{},{},{}\n",
                        r.proposal,
                        r.capture,
                        r.sequence,
                        q.phase as u8,
                        q.edge,
                        q.approach[0],
                        q.approach[1],
                        q.target[0],
                        q.target[1],
                        q.target[2]
                    ));
                }
                out
            }
        }
    }
    pub(super) fn read(raw: &str, repeated: bool) -> Result<Self, String> {
        let mut lines = raw.lines();
        if lines.next() != Some(if repeated { REPEATS } else { COLUMNS }) {
            return Err("The follow-up row header differs from its plan version. Preserve the original and re-export its observation analysis.".into());
        }
        if repeated {
            let mut rows = Vec::new();
            for (i, line) in lines.enumerate() {
                let fields = line.split(',').collect::<Vec<_>>();
                let error = |name: &str| {
                    format!("Top repeat row {i} has an invalid {name}. Preserve the plan and re-export the original observation analysis.")
                };
                let [proposal, capture, sequence, phase, edge, ax, ay, tx, ty, tz] =
                    fields.as_slice()
                else {
                    return Err(error("field count"));
                };
                let number = |raw: &str, name: &str| {
                    raw.parse::<f64>()
                        .ok()
                        .filter(|n| n.is_finite())
                        .ok_or_else(|| error(name))
                };
                rows.push(Repeat {
                    proposal: proposal.parse().map_err(|_| error("proposal ID"))?,
                    capture: (*capture).into(),
                    sequence: sequence.parse().map_err(|_| error("source sequence"))?,
                    request: Request {
                        phase: Phase::read(number(phase, "phase")?)?,
                        edge: edge.parse().map_err(|_| error("edge"))?,
                        approach: [number(ax, "approach X")?, number(ay, "approach Y")?],
                        target: [
                            number(tx, "target X")?,
                            number(ty, "target Y")?,
                            number(tz, "target Z")?,
                        ],
                    },
                });
            }
            Ok(Self::Repeats(rows))
        } else {
            lines.map(|line| {
                let error = || "A follow-up XY row is malformed. Re-export the original observation analysis.".to_string();
                let (x, y) = line.split_once(',').ok_or_else(error)?;
                Ok([x.parse().map_err(|_| error())?, y.parse().map_err(|_| error())?])
            }).collect::<Result<Vec<_>, String>>().map(Self::Columns)
        }
    }
    pub(super) fn read_directed(raw: &str) -> Result<Self, String> {
        let mut lines = raw.lines();
        let error = || {
            "Malformed adaptive observation rows. Preserve the plan and regenerate it from its retained request; Abort then Pendant Mode for an active run.".to_string()
        };
        if lines.next() != Some(DIRECTED) {
            return Err(error());
        }
        let rows = lines
            .map(|line| {
                let v = line.split(',').collect::<Vec<_>>();
                if v.len() != 7 {
                    return Err(error());
                }
                let number = |i: usize| {
                    v[i].parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(error)
                };
                Ok(Request {
                    phase: Phase::read(number(0)?)?,
                    edge: v[1].parse().map_err(|_| error())?,
                    approach: [number(2)?, number(3)?],
                    target: [number(4)?, number(5)?, number(6)?],
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self::Directed(rows))
    }
}
