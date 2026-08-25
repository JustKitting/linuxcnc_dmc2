mod config;
mod domains;
mod generator;
mod macro_inventory;
mod macro_probe;
mod parser;
mod probe;
mod process;
mod source;

pub(crate) fn run() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build");
    generator::generate();
}
