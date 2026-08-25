#[path = "build/bindings.rs"]
mod bindings;
#[path = "build/config.rs"]
mod config;
#[path = "build/probe.rs"]
mod probe;
#[path = "build/process.rs"]
mod process;
#[path = "build/return_contract.rs"]
mod return_contract;
#[path = "build/source.rs"]
mod source;

use std::env;
use std::path::PathBuf;

fn main() {
    for input in [
        "build.rs",
        "build/bindings.rs",
        "build/config.rs",
        "build/probe.rs",
        "build/process.rs",
        "build/return_contract.rs",
        "build/source.rs",
        "src/abi.rs",
        "src/return_code.rs",
    ] {
        println!("cargo:rerun-if-changed={input}");
    }

    source::verify_audited_source();
    return_contract::verify_audited_returns();
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    probe::compile_c_abi_contract(&output_directory);
    bindings::generate(&output_directory);
}
