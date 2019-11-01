use ckb_crypto::secp::SECP256K1;
use ckb_hash::blake2b_256;
use ckb_jsonrpc_types::{Script as RpcScript, Transaction as RpcTransaction};
use ckb_sdk::{Address, GenesisInfo, HttpRpcClient, NetworkType, OldAddress};
use ckb_types::{packed, prelude::*, H160};
use clap::{App, Arg, ArgMatches, SubCommand};
use faster_hex::hex_string;
use std::fs;
use std::path::PathBuf;

use super::CliSubCommand;
use crate::utils::{
    arg_parser::{
        AddressParser, ArgParser, FilePathParser, FixedHashParser, HexParser, PrivkeyPathParser,
        PubkeyHexParser,
    },
    other::{get_address, get_genesis_info},
    printer::{OutputFormat, Printable},
};
use std::fs::File;
use std::io::{BufReader, BufRead, Write};

pub struct UtilSubCommand<'a> {
    rpc_client: &'a mut HttpRpcClient,
    genesis_info: Option<GenesisInfo>,
}

impl<'a> UtilSubCommand<'a> {
    pub fn new(
        rpc_client: &'a mut HttpRpcClient,
        genesis_info: Option<GenesisInfo>,
    ) -> UtilSubCommand<'a> {
        UtilSubCommand {
            rpc_client,
            genesis_info,
        }
    }

    pub fn subcommand(name: &'static str) -> App<'static, 'static> {
        let arg_privkey = Arg::with_name("privkey-path")
            .long("privkey-path")
            .takes_value(true)
            .validator(|input| PrivkeyPathParser.validate(input))
            .help("Private key file path (only read first line)");
        let arg_pubkey = Arg::with_name("pubkey")
            .long("pubkey")
            .takes_value(true)
            .validator(|input| PubkeyHexParser.validate(input))
            .help("Public key (hex string, compressed format)");
        let arg_address = Arg::with_name("address")
            .long("address")
            .takes_value(true)
            .validator(|input| AddressParser.validate(input))
            .required(true)
            .help("Target address (see: https://github.com/nervosnetwork/ckb/wiki/Common-Address-Format)");
        let arg_lock_arg = Arg::with_name("lock-arg")
            .long("lock-arg")
            .takes_value(true)
            .validator(|input| FixedHashParser::<H160>::default().validate(input))
            .help("Lock argument (account identifier, blake2b(pubkey)[0..20])");

        let json_path_arg = Arg::with_name("json-path")
            .long("json-path")
            .takes_value(true)
            .required(true)
            .validator(|input| FilePathParser::new(true).validate(input));
        let file_path_arg = Arg::with_name("file")
            .long("file")
            .takes_value(true)
            .required(true)
            .validator(|input| FilePathParser::new(true).validate(input));
        let binary_hex_arg = Arg::with_name("binary-hex")
            .long("binary-hex")
            .takes_value(true)
            .required(true)
            .validator(|input| HexParser.validate(input));
        let serialize_output_type_arg = Arg::with_name("output-type")
            .long("output-type")
            .takes_value(true)
            .default_value("binary")
            .possible_values(&["binary", "hash"])
            .help("Serialize output type");
        SubCommand::with_name(name)
            .about("Utilities")
            .subcommands(vec![
                SubCommand::with_name("convert-addr")
                    .about(
                        "convert addresses from file",
                    )
                    .arg(file_path_arg.clone().required(true)),
                SubCommand::with_name("addr-info")
                    .about(
                        "Print different types of address",
                    )
                    .arg(arg_address.clone().required(true)),
                SubCommand::with_name("key-info")
                    .about(
                        "Show public information of a secp256k1 private key (from file) or public key",
                    )
                    .arg(arg_privkey.clone().conflicts_with("pubkey"))
                    .arg(arg_pubkey.clone().required(false))
                    .arg(arg_address.clone().required(false))
                    .arg(arg_lock_arg.clone()),
                SubCommand::with_name("serialize-tx")
                    .about("Serialize a transaction from json file to hex binary or hash")
                    .arg(json_path_arg.clone()
                         .help("Transaction content (json format, without witnesses/hash, see rpc get_transaction)"))
                    .arg(serialize_output_type_arg.clone()),
                SubCommand::with_name("deserialize-tx")
                    .about("Deserialize a transaction from binary hex to json")
                    .arg(binary_hex_arg.clone().help("Transaction binary hex")),
                SubCommand::with_name("serialize-script")
                    .about("Serialize a script from json file to hex binary or hash")
                    .arg(json_path_arg.clone()
                         .help("Script content (json format, see rpc get_transaction)"))
                    .arg(serialize_output_type_arg.clone()),
                SubCommand::with_name("deserialize-script")
                    .about("Deserialize a script from hex binary to json")
                    .arg(binary_hex_arg.clone().help("Script binary hex")),
            ])
    }
}

fn batch_addr_info(ifile: &str) -> String {
    let is_epoch_reward_file = ifile.contains("epoch_reward");
    let ofile_name = format!("{}.out.csv", ifile);
    let mut ofile = File::create(&ofile_name).expect("open output file");
    let parser = AddressParser{};
    BufReader::new(File::open(ifile).expect("open input file"))
        .lines()
        .for_each(|line| {
            if let Ok(line) = line {
                let splits = line.split(',').collect::<Vec<_>>();
                let addr = if is_epoch_reward_file {
                    &splits[1]
                } else {
                    &splits[0]
                };
                if (addr.len() != 46 && addr.len() != 50) || &addr[0..2] != "ck" {
                    return;
                }
                let parsed = parser.parse(addr).expect("parse addr");

                writeln!(ofile, "{},{}", addr, parsed.to_string(NetworkType::MainNet)).unwrap();
//                if is_epoch_reward_file {
//                    writeln!(ofile, "{},{},{}", splits[0], addr.to_string(NetworkType::MainNet), splits[2..].join(",")).unwrap();
//                } else {
//                    writeln!(ofile, "{},{}", addr.to_string(NetworkType::MainNet), splits[1..].join(",")).unwrap();
//                }
            }
        });

    ofile_name
}

impl<'a> CliSubCommand for UtilSubCommand<'a> {
    fn process(
        &mut self,
        matches: &ArgMatches,
        format: OutputFormat,
        color: bool,
    ) -> Result<String, String> {
        match matches.subcommand() {
            ("convert-addr", Some(m)) => {
                let ifile = m.value_of("file").expect("original file is required");
                let ofile = batch_addr_info(ifile);
                let resp = serde_json::json!({
                    "input": ifile,
                    "output": ofile,
                });
                Ok(resp.render(format, color))
            },
            ("addr-info", Some(m)) => {
                let address = get_address(m)?;
                let old_address = OldAddress::new_default(address.hash().clone());
                let resp = serde_json::json!({
                    "new_ckt_address": address.to_string(NetworkType::TestNet),
                    "new_ckb_address": address.to_string(NetworkType::MainNet),
                    "old_ckt_address": old_address.to_string(NetworkType::TestNet),
                    "old_ckb_address": old_address.to_string(NetworkType::MainNet),
                });
                Ok(resp.render(format, color))
            },
            ("key-info", Some(m)) => {
                let privkey_opt: Option<secp256k1::SecretKey> =
                    PrivkeyPathParser.from_matches_opt(m, "privkey-path", false)?;
                let pubkey_opt: Option<secp256k1::PublicKey> =
                    PubkeyHexParser.from_matches_opt(m, "pubkey", false)?;
                let pubkey_opt = privkey_opt
                    .map(|privkey| secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey))
                    .or_else(|| pubkey_opt);
                let pubkey_string_opt = pubkey_opt.as_ref().map(|pubkey| {
                    hex_string(&pubkey.serialize()[..]).expect("encode pubkey failed")
                });
                let address = match pubkey_opt {
                    Some(pubkey) => {
                        let pubkey_hash = blake2b_256(&pubkey.serialize()[..]);
                        Address::from_lock_arg(&pubkey_hash[0..20])?
                    }
                    None => get_address(m)?,
                };
                let old_address = OldAddress::new_default(address.hash().clone());
                let genesis_info = get_genesis_info(&mut self.genesis_info, self.rpc_client)?;
                let secp_type_hash = genesis_info.secp_type_hash();
                println!(
                    r#"Put this config in < ckb.toml >:

[block_assembler]
code_hash = "{:#x}"
hash_type = "type"
args = ["{:#x}"]
"#,
                    secp_type_hash,
                    address.hash()
                );

                let resp = serde_json::json!({
                    "pubkey": pubkey_string_opt,
                    "address": {
                        "testnet": address.to_string(NetworkType::TestNet),
                        "mainnet": address.to_string(NetworkType::MainNet),
                    },
                    // NOTE: remove this later (after all testnet race reward received)
                    "old-testnet-address": old_address.to_string(NetworkType::TestNet),
                    "lock_arg": format!("{:x}", address.hash()),
                    "lock_hash": address.lock_script(secp_type_hash.clone()).calc_script_hash(),
                });
                Ok(resp.render(format, color))
            }
            ("serialize-tx", Some(m)) => {
                let json_path: PathBuf = FilePathParser::new(true).from_matches(m, "json-path")?;
                let content = fs::read_to_string(json_path).map_err(|err| err.to_string())?;
                let rpc_tx: RpcTransaction =
                    serde_json::from_str(&content).map_err(|err| err.to_string())?;
                let tx: packed::Transaction = rpc_tx.into();
                let output = match m.value_of("output-type") {
                    Some("binary") => hex_string(tx.raw().as_slice()).unwrap(),
                    Some("hash") => format!("{:#x}", tx.calc_tx_hash()),
                    _ => panic!("Invalid output type"),
                };
                Ok(output)
            }
            ("deserialize-tx", Some(m)) => {
                let binary: Vec<u8> = HexParser.from_matches(m, "binary-hex")?;
                let raw_tx =
                    packed::RawTransaction::from_slice(&binary).map_err(|err| err.to_string())?;
                let rpc_tx: RpcTransaction = packed::Transaction::new_builder()
                    .raw(raw_tx)
                    .build()
                    .into();
                Ok(rpc_tx.render(format, color))
            }
            ("serialize-script", Some(m)) => {
                let json_path: PathBuf = FilePathParser::new(true).from_matches(m, "json-path")?;
                let content = fs::read_to_string(json_path).map_err(|err| err.to_string())?;
                let rpc_script: RpcScript =
                    serde_json::from_str(&content).map_err(|err| err.to_string())?;
                let script: packed::Script = rpc_script.into();
                let output = match m.value_of("output-type") {
                    Some("binary") => hex_string(script.as_slice()).unwrap(),
                    Some("hash") => format!("{:#x}", script.calc_script_hash()),
                    _ => panic!("Invalid output type"),
                };
                Ok(output)
            }
            ("deserialize-script", Some(m)) => {
                let binary: Vec<u8> = HexParser.from_matches(m, "binary-hex")?;
                let rpc_script: RpcScript = packed::Script::from_slice(&binary)
                    .map_err(|err| err.to_string())?
                    .into();
                Ok(rpc_script.render(format, color))
            }
            _ => Err(matches.usage().to_owned()),
        }
    }
}
