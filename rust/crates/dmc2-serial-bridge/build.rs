use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(program: &str, arguments: &[&str]) {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn main() {
    println!("cargo:rerun-if-changed=src/application/serial/posix_ffi.h");
    println!("cargo:rerun-if-changed=build.rs");
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let bindings = output_directory.join("serial_posix_bindings.rs");
    let bindings_text = bindings
        .to_str()
        .expect("Cargo output path is not valid UTF-8");
    run(
        "bindgen",
        &[
            "src/application/serial/posix_ffi.h",
            "--allowlist-function",
            "(__errno_location|open|close|read|tcgetattr|cfmakeraw|cfsetispeed|cfsetospeed|tcsetattr|tcflush)",
            "--allowlist-type",
            "(termios|speed_t|tcflag_t|cc_t|ssize_t)",
            "--allowlist-var",
            "(EAGAIN|EWOULDBLOCK|EINTR|EIO|EINVAL|O_RDWR|O_NOCTTY|O_NONBLOCK|O_CLOEXEC|B115200|CLOCAL|CREAD|CSTOPB|CRTSCTS|CSIZE|CS8|VMIN|VTIME|TCSANOW|TCIFLUSH)",
            "--use-core",
            "--no-layout-tests",
            "--output",
            bindings_text,
        ],
    );
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-lib=util");
    println!("cargo:rustc-link-lib=linuxcnchal");
}
