mod application;

fn main() {
    if let Err(error) = application::run() {
        eprintln!("dmc2-serial-bridge: {error}");
        std::process::exit(1);
    }
}
