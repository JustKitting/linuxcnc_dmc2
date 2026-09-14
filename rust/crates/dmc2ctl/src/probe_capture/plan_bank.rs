//! Data parameters owned by stock AXIS integration, never motion/control pins.
use dmc2_hal_sys as hal;
use std::ffi::CString;

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub name: &'static str,
    pub fields: &'static str,
}
pub(super) struct Bank {
    id: i32,
    spec: Spec,
}

impl Bank {
    fn set(&self, field: &str, value: f64) -> Result<(), String> {
        if !self.spec.fields.lines().skip(1).any(|name| name == field) || !value.is_finite() {
            return Err(format!(
                "Invalid probe plan field {field}; no plan was published."
            ));
        }
        let name = CString::new(format!("axisui.{}-plan-{field}", self.spec.name))
            .map_err(|e| e.to_string())?;
        let result = unsafe { hal::hal_param_float_set(name.as_ptr(), value) };
        if result != 0 {
            return Err(format!("AXIS did not accept probe plan field {field} (HAL return {result}). Reopen the standard CNC application so its Custom Scripts parameters are present."));
        }
        Ok(())
    }

    pub fn invalidate(&self) -> Result<(), String> {
        self.set("sequence", -1.0)
    }

    pub fn publish(&self, values: &[(&str, f64)]) -> Result<(), String> {
        let expected: Vec<_> = self.spec.fields.lines().skip(1).collect();
        if values.iter().map(|p| p.0).collect::<Vec<_>>() != expected {
            return Err("The probe planner and UI data contract differ. Rebuild/reopen the matching application.".into());
        }
        // Invalidate first, commit sequence last. M190 + M66 is the interpreter
        // read-ahead barrier. An error cannot leave an old plan looking current.
        self.invalidate()?;
        for &(key, value) in &values[1..] {
            self.set(key, value)?;
        }
        self.set("sequence", values[0].1)
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        // Removes only this temporary client's component. The data belongs to
        // AXIS and survives. No reset, fault clear or task command is issued.
        if unsafe { hal::hal_exit(self.id) } != 0 {
            eprintln!("Probe plan client could not detach from HAL. Abort then Pendant Mode if a script is still active.");
        }
    }
}

pub(super) fn with_bank<T>(
    spec: Spec,
    f: impl FnOnce(&Bank) -> Result<T, String>,
) -> Result<T, String> {
    let name = CString::new(format!("dmc2-{}-plan-{}", spec.name, std::process::id())).unwrap();
    let id = unsafe { hal::hal_init(name.as_ptr()) };
    if id < 0 {
        return Err(format!("Cannot connect to the probe plan data bank (HAL return {id}). The CNC application must be running."));
    }
    f(&Bank { id, spec })
}
