//! Gauge-block binding to the shared AXIS data-only plan bank.
use super::super::plan_bank::{self, Bank, Spec};
const SPEC: Spec = Spec {
    name: "block",
    fields: include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../config/block-plan-fields.txt"
    )),
};
pub(super) fn with_bank<T>(f: impl FnOnce(&Bank) -> Result<T, String>) -> Result<T, String> {
    plan_bank::with_bank(SPEC, f)
}
pub(crate) fn invalidate() -> Result<(), String> {
    with_bank(Bank::invalidate)
}
