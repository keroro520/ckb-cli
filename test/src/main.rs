mod miner;
mod spec;
mod util;

use std::env;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::spec::{
    PrepareAtEndOfFirstPeriod, PrepareInFirstPeriod, PrepareInSecondPeriod, Setup, Spec,
};
use crate::util::run_cmd;
use clap::{App, Arg};
use tempfile::{tempdir, TempDir};

// TODO dynamic rpc port
const RPC_PORT: u16 = 8114;
const P2P_PORT: u16 = 9114;

fn main() {
    let _ = {
        let filter = env::var("CKB_LOG").unwrap_or_else(|_| "info".to_string());
        env_logger::builder().parse_filters(&filter).try_init()
    };
    let matches = App::new("ckb-cli-test")
        .arg(
            Arg::with_name("ckb-bin")
                .long("ckb-bin")
                .takes_value(true)
                .required(true)
                .value_name("PATH")
                .help("Path to ckb executable"),
        )
        .arg(
            Arg::with_name("cli-bin")
                .long("cli-bin")
                .takes_value(true)
                .required(true)
                .value_name("PATH")
                .help("Path to ckb-cli executable"),
        )
        .get_matches();
    let ckb_bin = matches.value_of("ckb-bin").unwrap();
    let cli_bin = matches.value_of("cli-bin").unwrap();
    assert!(
        Path::new(ckb_bin).exists(),
        "ckb binary not exists: {}",
        ckb_bin
    );
    assert!(
        Path::new(cli_bin).exists(),
        "ckb-cli binary not exists: {}",
        cli_bin
    );

    for spec in all_specs() {
        run_spec(spec, ckb_bin, cli_bin);
    }
}

fn run_spec(spec: Box<dyn Spec>, ckb_bin: &str, cli_bin: &str) {
    let (tmpdir, ckb_dir) = temp_dir();
    let _stdout = run_cmd(
        ckb_bin,
        vec![
            "-C",
            ckb_dir.as_str(),
            "init",
            "--chain",
            "dev",
            "--rpc-port",
            &RPC_PORT.to_string(),
            "--p2p-port",
            &P2P_PORT.to_string(),
        ],
    );

    let setup = Setup {
        ckb_dir,
        ckb_bin: ckb_bin.to_string(),
        cli_bin: cli_bin.to_string(),
        rpc_port: RPC_PORT,
    };
    setup.modify_ckb_toml(&*spec);
    setup.modify_spec_toml(&*spec);

    let child_process = Command::new(&setup.ckb_bin)
        .env("RUST_BACKTRACE", "full")
        .args(&["-C", &setup.ckb_dir, "run", "--ba-advanced"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Run `ckb run` failed");
    let _guard = ProcessGuard(child_process);

    spec.run(&setup);

    tmpdir.close().expect("Close tmp dir failed");
}

fn all_specs() -> Vec<Box<dyn Spec>> {
    vec![
        Box::new(PrepareInFirstPeriod),
        Box::new(PrepareAtEndOfFirstPeriod),
        Box::new(PrepareInSecondPeriod),
    ]
}

struct ProcessGuard(pub Child);

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        match self.0.kill() {
            Err(e) => log::error!("Could not kill ckb process: {}", e),
            Ok(_) => log::debug!("Successfully killed ckb process"),
        }
        let _ = self.0.wait();
    }
}

pub fn temp_dir() -> (TempDir, String) {
    let tempdir = tempdir().expect("create tempdir failed");
    let path = tempdir.path().to_str().unwrap().to_owned();
    (tempdir, path)
}
