use std::net::TcpListener;
use std::process::Command;

pub fn run_cmd(bin: &str, args: Vec<&str>) -> String {
    log::info!("[Execute]: {} {:?}", bin, args.join(" "));
    let init_output = Command::new(bin.to_owned())
        .env("RUST_BACKTRACE", "full")
        .args(&args)
        .output()
        .expect("Run command failed");

    if !init_output.status.success() {
        log::error!("{}", String::from_utf8_lossy(init_output.stderr.as_slice()));
        panic!("Fail to execute command");
    }
    String::from_utf8_lossy(init_output.stdout.as_slice()).to_string()
}

pub fn find_available_port(start: u16, end: u16) -> u16 {
    for port in start..=end {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    unreachable!()
}
