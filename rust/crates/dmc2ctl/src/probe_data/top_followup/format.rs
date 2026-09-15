use super::{data, number, rows::identifier, Plan, Role, Rows, START_FIELDS};
const MAGIC: &str = "DMC2_TOP_FOLLOWUP_V1";
const ROLE_MAGIC: &str = "DMC2_TOP_FOLLOWUP_V2";
const REPEAT_MAGIC: &str = "DMC2_TOP_FOLLOWUP_V3";
const START: &str = "DMC2_TOP_FOLLOWUP_START_V1";
const IDS: [&str; 4] = ["object", "setup", "analysis", "capture"];

impl Plan {
    pub fn encode(&self) -> Result<String, String> {
        self.settings()?;
        let mut out = format!(
            "{}\n",
            if self.rows.repeated() {
                REPEAT_MAGIC
            } else if self.role.is_some() {
                ROLE_MAGIC
            } else {
                MAGIC
            }
        );
        for (name, value) in IDS.into_iter().zip(&self.source) {
            identifier(name, value)?;
            out.push_str(&format!("{name}={value}\n"));
        }
        if let Some(role) = self.role {
            out.push_str(&format!("contact_role={}\n", role.name()));
        }
        out.push_str(&format!("\n{START}\n"));
        for &key in START_FIELDS {
            number(&self.start, key)?;
            out.push_str(&format!("{key}={}\n", self.start[key]));
        }
        for section in [&self.plate, &self.feeds] {
            out.push('\n');
            out.push_str(section.trim_end_matches('\n'));
            out.push('\n');
        }
        out.push('\n');
        out.push_str(&self.rows.encode());
        Ok(out)
    }

    pub fn read(raw: &str) -> Result<Self, String> {
        let error = || {
            "The retained top follow-up plan is malformed. Preserve it and re-export from the original observation analysis; Abort then Pendant Mode for an active run.".to_string()
        };
        let sections = raw.split("\n\n").collect::<Vec<_>>();
        let [identity, start, plate, feeds, rows] = sections.as_slice() else {
            return Err(error());
        };
        let mut identity = identity.lines();
        let version = identity.next().ok_or_else(error)?;
        if !matches!(version, MAGIC | ROLE_MAGIC | REPEAT_MAGIC) {
            return Err(error());
        }
        let mut source: [String; 4] = Default::default();
        for (name, value) in IDS.into_iter().zip(&mut source) {
            *value = identity
                .next()
                .and_then(|s| s.strip_prefix(&format!("{name}=")))
                .ok_or_else(error)?
                .into();
        }
        let role = if version != MAGIC {
            Some(Role::read(
                identity
                    .next()
                    .and_then(|s| s.strip_prefix("contact_role="))
                    .ok_or_else(|| "The retained top follow-up plan has no contact_role field. Preserve it and re-export from the original observation analysis; Abort then Pendant Mode for an active run.".to_string())?,
            )?)
        } else {
            None
        };
        if identity.next().is_some() {
            return Err(error());
        }
        let rows = Rows::read(rows, version == REPEAT_MAGIC)?;
        let plan = Self {
            role,
            source,
            start: data(start, START, START_FIELDS)?,
            plate: format!("{plate}\n"),
            feeds: format!("{feeds}\n"),
            rows,
        };
        if plan.encode()? != raw {
            return Err(error());
        }
        Ok(plan)
    }
}
