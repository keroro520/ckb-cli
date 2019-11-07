use crate::subcommands::dao::builder::DAOBuilder;
use crate::subcommands::functional::{
    build_secp_witnesses, can_deposit, can_prepare, can_withdraw, sign_with_keystore,
    sign_with_privkey, ChainClient, IndexClient,
};
use crate::utils::arg_parser::{
    ArgParser, CapacityParser, FixedHashParser, PrivkeyPathParser, PrivkeyWrapper,
};
use crate::utils::other::{get_address, read_password};
use crate::utils::printer::{OutputFormat, Printable};
use ckb_hash::blake2b_256;
use ckb_index::LiveCellInfo;
use ckb_sdk::{Address, SECP256K1};
use ckb_types::core::TransactionView;
use ckb_types::packed::{Byte32, CellOutput};
use ckb_types::prelude::*;
use ckb_types::{H160, H256};
use clap::ArgMatches;
use itertools::Itertools;
use std::collections::HashSet;

mod builder;
mod command;

// TODO optimize the cell-take strategy
// TODO check whether output data is empty before spending
// TODO Allow transaction change

pub struct DAOSubCommand<'a> {
    chain_client: &'a mut ChainClient<'a>,
    index_client: &'a mut IndexClient<'a>,
    // output_format, color, debug
    output_style: (OutputFormat, bool, bool),
    // from_privkey, from_account, from_address, with_password, capacity, tx_fee
    transfer_args: Option<(
        Option<PrivkeyWrapper>,
        Option<H160>,
        Address,
        bool,
        u64,
        u64,
    )>,
}

impl<'a> DAOSubCommand<'a> {
    pub fn new(
        chain_client: &'a mut ChainClient<'a>,
        index_client: &'a mut IndexClient<'a>,
    ) -> Self {
        Self {
            chain_client,
            index_client,
            output_style: (OutputFormat::Yaml, true, true),
            transfer_args: None,
        }
    }

    pub fn deposit(
        &mut self,
        m: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        self.output_style = (format, color, debug);
        self.transfer_args = Some(transfer_args(m)?);
        self.check_db_ready()?;

        let genesis_info = self.chain_client.genesis_info()?;
        let secp_type_hash = self.chain_client.secp_type_hash()?;
        let network_type = self.chain_client.network_type()?;
        let from_address = self.transfer_args.as_ref().unwrap().2.clone();
        let target_capacity = {
            let transfer_args = self.transfer_args.as_ref().expect("checked above");
            transfer_args.4 + transfer_args.5
        };
        let mut take_capacity = 0;
        let mut enough = false;
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let terminator = |_, info: &LiveCellInfo| {
            if Ok(true) != can_deposit(chain_client, info) {
                return (false, false);
            }

            let (stop, take) = if take_capacity + info.capacity == target_capacity {
                (true, true)
            } else {
                (false, false)
            };
            if take {
                take_capacity += info.capacity;
            }
            if stop {
                enough = true;
            }

            (stop, take)
        };

        let infos: Vec<LiveCellInfo> = {
            index_client.with_db(network_type, genesis_info, |db| {
                db.get_live_cells_by_lock(
                    from_address
                        .lock_script(secp_type_hash.clone())
                        .calc_script_hash(),
                    None,
                    terminator,
                )
            })?
        };

        if !enough {
            return Err(format!(
                "Capacity not enough: {} => {}",
                from_address.to_string(network_type),
                take_capacity,
            ));
        }

        let tx = self.build(infos).deposit(self.chain_client)?;
        let tx = self.sign(tx)?;
        self.send_transaction(tx)
    }

    pub fn prepare(
        &mut self,
        m: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        self.output_style = (format, color, debug);
        self.transfer_args = Some(transfer_args(m)?);
        self.check_db_ready()?;

        {
            let transfer_args = self.transfer_args.as_ref().expect("checked above");
            let tx_fee = transfer_args.5;
            assert_eq!(tx_fee, 0);
        };

        let genesis_info = self.chain_client.genesis_info()?;
        let secp_type_hash = self.chain_client.secp_type_hash()?;
        let network_type = self.chain_client.network_type()?;
        let from_address = self.transfer_args.as_ref().unwrap().2.clone();
        let target_capacity = {
            let transfer_args = self.transfer_args.as_ref().expect("checked above");
            transfer_args.4
        };
        let mut take_capacity = 0;
        let mut enough = false;
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let terminator = |_, info: &LiveCellInfo| {
            if Ok(true) != can_prepare(chain_client, info) {
                return (false, false);
            }

            let (stop, take) = if take_capacity + info.capacity == target_capacity {
                (true, true)
            } else {
                (false, false)
            };
            if take {
                take_capacity += info.capacity;
            }
            if stop {
                enough = true;
            }

            (stop, take)
        };

        let infos: Vec<LiveCellInfo> = {
            index_client
                .with_db(network_type, genesis_info, |db| {
                    db.get_live_cells_by_lock(
                        from_address
                            .lock_script(secp_type_hash.clone())
                            .calc_script_hash(),
                        None,
                        terminator,
                    )
                })
                .map_err(|_err| {
                    format!(
                        "index database may not ready, sync process: {}",
                        self.index_client.state()
                    )
                })?
        };

        if !enough {
            let network_type = self.chain_client.network_type()?;
            return Err(format!(
                "Capacity not enough: {} => {}",
                from_address.to_string(network_type),
                take_capacity,
            ));
        }

        let tx = self.build(infos).prepare(self.chain_client)?;
        let tx = self.sign(tx)?;
        self.send_transaction(tx)
    }

    pub fn withdraw(
        &mut self,
        m: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        self.output_style = (format, color, debug);
        self.transfer_args = Some(transfer_args(m)?);
        self.check_db_ready()?;

        let genesis_info = self.chain_client.genesis_info()?;
        let secp_type_hash = self.chain_client.secp_type_hash()?;
        let network_type = self.chain_client.network_type()?;
        let from_address = self.transfer_args.as_ref().unwrap().2.clone();
        let target_capacity = {
            let transfer_args = self.transfer_args.as_ref().expect("checked above");
            transfer_args.4 + transfer_args.5
        };
        let mut take_capacity = 0;
        let mut enough = false;
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let terminator = |_, info: &LiveCellInfo| {
            if Ok(true) != can_withdraw(chain_client, info) {
                return (false, false);
            }

            let max_withdrawal: u64 = chain_client
                .calculate_dao_maximum_withdraw(&info)
                .expect("RPC calculate_dao_maximum_withdraw for a prepare cell");
            let (stop, take) = if take_capacity + max_withdrawal == target_capacity {
                (true, true)
            } else {
                (false, false)
            };
            if take {
                take_capacity += max_withdrawal
            }
            if stop {
                enough = true;
            }

            (stop, take)
        };

        let infos: Vec<LiveCellInfo> = {
            index_client
                .with_db(network_type, genesis_info, |db| {
                    db.get_live_cells_by_lock(
                        from_address
                            .lock_script(secp_type_hash.clone())
                            .calc_script_hash(),
                        None,
                        terminator,
                    )
                })
                .map_err(|_err| {
                    format!(
                        "index database may not ready, sync process: {}",
                        self.index_client.state(),
                    )
                })?
        };

        if !enough {
            return Err(format!(
                "Capacity not enough: {} => {}",
                from_address.to_string(network_type),
                take_capacity,
            ));
        }

        let tx = self.build(infos).withdraw(self.chain_client)?;
        let tx = self.sign(tx)?;
        self.send_transaction(tx)
    }

    pub fn query_live_deposited_cells(
        &mut self,
        m: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        self.output_style = (format, color, debug);
        let chain_client = &mut self.chain_client;
        let network_type = chain_client.network_type()?;
        let genesis_info = chain_client.genesis_info()?;
        let dao_type_hash = chain_client.dao_type_hash()?;
        let lock_hash = query_args(m, chain_client)?;
        let infos = self
            .index_client
            .with_db(network_type, genesis_info, |db| {
                let infos_by_lock = db
                    .get_live_cells_by_lock(lock_hash, Some(0), |_, _| (false, true))
                    .into_iter()
                    .collect::<HashSet<_>>();
                let infos_by_code = db
                    .get_live_cells_by_code(dao_type_hash.clone(), Some(0), |_, _| (false, true))
                    .into_iter()
                    .collect::<HashSet<_>>();
                infos_by_lock
                    .intersection(&infos_by_code)
                    .filter(|info| can_prepare(chain_client, info).unwrap_or(false))
                    .sorted_by_key(|live| (live.number, live.tx_index, live.index.output_index))
                    .cloned()
                    .collect::<Vec<_>>()
            })?;
        let total_capacity = infos.iter().map(|live| live.capacity).sum::<u64>();
        let resp = serde_json::json!({
            "live_cells": infos.into_iter().map(|info| {
                serde_json::to_value(&info).unwrap()
            }).collect::<Vec<_>>(),
            "total_capacity": total_capacity,
        });
        Ok(resp.render(format, color))
    }

    pub fn query_live_prepared_cells(
        &mut self,
        m: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        self.output_style = (format, color, debug);
        let chain_client = &mut self.chain_client;
        let network_type = chain_client.network_type()?;
        let genesis_info = chain_client.genesis_info()?;
        let dao_type_hash = chain_client.dao_type_hash()?;
        let lock_hash = query_args(m, chain_client)?;
        let infos: Vec<LiveCellInfo> =
            self.index_client
                .with_db(network_type, genesis_info, |db| {
                    let infos_by_lock = db
                        .get_live_cells_by_lock(lock_hash, Some(0), |_, _| (false, true))
                        .into_iter()
                        .collect::<HashSet<_>>();
                    let infos_by_code = db
                        .get_live_cells_by_code(dao_type_hash.clone(), Some(0), |_, _| {
                            (false, true)
                        })
                        .into_iter()
                        .collect::<HashSet<_>>();
                    infos_by_lock
                        .intersection(&infos_by_code)
                        .filter(|info| can_withdraw(chain_client, info).unwrap_or(false))
                        .sorted_by_key(|live| (live.number, live.tx_index, live.index.output_index))
                        .cloned()
                        .collect::<Vec<_>>()
                })?;
        let total_capacity = infos.iter().map(|live| live.capacity).sum::<u64>();
        let mut total_maximum_withdraw = 0;
        let infos_with_interest = infos.into_iter().map(|info| {
            let maximum_withdraw = chain_client
                .calculate_dao_maximum_withdraw(&info)
                .expect("calculate_dao_maximum_withdraw failed; TODO");
            total_maximum_withdraw += maximum_withdraw;
            (info, maximum_withdraw)
        });

        let resp = serde_json::json!({
            "live_cells": infos_with_interest.into_iter().map(|(info, maximum_withdraw)| {
                let mut value = serde_json::to_value(&info).unwrap();
                let obj = value.as_object_mut().unwrap();
                obj.insert("maximum_withdraw".to_owned(), serde_json::json!(maximum_withdraw));
                value
            }).collect::<Vec<_>>(),
            "total_capacity": total_capacity,
            "total_maximum_withdraw": total_maximum_withdraw,
        });
        Ok(resp.render(format, color))
    }

    fn build(&self, infos: Vec<LiveCellInfo>) -> DAOBuilder {
        let tx_fee = self.transfer_args.as_ref().unwrap().5;
        DAOBuilder::new(tx_fee, infos)
    }

    fn sign(&mut self, transaction: TransactionView) -> Result<TransactionView, String> {
        let (from_key, from_account, from_address, with_password, _capacity, _tx_fee) =
            self.transfer_args.as_ref().unwrap().clone();
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let secp_cell_dep = chain_client.genesis_info()?.secp_dep();
        let secp_type_hash = chain_client.secp_type_hash()?;
        let interactive = index_client.interactive();

        // Build outputs' SECP locks, and remember putting SECP cell into cell_deps
        let outputs = transaction
            .outputs()
            .into_iter()
            .map(|output: CellOutput| {
                output
                    .as_builder()
                    .lock(from_address.lock_script(secp_type_hash.clone()))
                    .build()
            })
            .collect::<Vec<_>>();
        let transaction = transaction
            .as_advanced_builder()
            .set_outputs(outputs)
            .cell_dep(secp_cell_dep)
            .build();

        // Sign SECP signature and put it into witness
        let tx_hash = transaction.hash();
        let witnesses = transaction
            .witnesses()
            .into_iter()
            .map(|w| w.unpack())
            .collect::<Vec<_>>();
        let witnesses = if let Some(privkey) = from_key.as_ref() {
            build_secp_witnesses(tx_hash, witnesses, |digest| {
                sign_with_privkey(privkey, digest)
            })?
        } else {
            let lock_arg = from_account.as_ref().unwrap();
            let password = if *with_password {
                Some(read_password(false, None)?)
            } else {
                None
            };
            build_secp_witnesses(tx_hash, witnesses, |digest| {
                sign_with_keystore(index_client, lock_arg, &password, interactive, digest)
            })?
        };

        Ok(transaction
            .as_advanced_builder()
            .set_witnesses(witnesses.iter().map(Pack::pack).collect())
            .build())
    }

    fn send_transaction(&mut self, transaction: TransactionView) -> Result<String, String> {
        let (format, color, debug) = &self.output_style;
        self.chain_client
            .send_transaction(transaction, *format, *debug, *color)
    }

    fn check_db_ready(&mut self) -> Result<(), String> {
        let network_type = self.chain_client.network_type()?;
        let genesis_info = self.chain_client.genesis_info()?;
        self.index_client
            .with_db(network_type, genesis_info, |_| ())
    }
}

fn transfer_args(
    m: &ArgMatches,
) -> Result<
    (
        Option<PrivkeyWrapper>,
        Option<H160>,
        Address,
        bool,
        u64,
        u64,
    ),
    String,
> {
    let from_privkey: Option<PrivkeyWrapper> =
        PrivkeyPathParser.from_matches_opt(m, "privkey-path", false)?;
    let from_account: Option<H160> =
        FixedHashParser::<H160>::default().from_matches_opt(m, "from-account", false)?;
    let from_address = if let Some(from_privkey) = from_privkey.as_ref() {
        let from_pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, from_privkey);
        let pubkey_hash = blake2b_256(&from_pubkey.serialize()[..]);
        Address::from_lock_arg(&pubkey_hash[0..20])?
    } else {
        Address::from_lock_arg(from_account.as_ref().unwrap().as_bytes())?
    };
    let capacity: u64 = CapacityParser.from_matches(m, "capacity")?;
    let tx_fee: u64 = CapacityParser.from_matches(m, "tx-fee")?;
    let with_password = m.is_present("with-password");

    Ok((
        from_privkey,
        from_account,
        from_address,
        with_password,
        capacity,
        tx_fee,
    ))
}

fn query_args(m: &ArgMatches, chain_client: &mut ChainClient) -> Result<Byte32, String> {
    let lock_hash_opt: Option<H256> =
        FixedHashParser::<H256>::default().from_matches_opt(m, "lock-hash", false)?;
    let lock_hash = if let Some(lock_hash) = lock_hash_opt {
        lock_hash.pack()
    } else {
        let secp_type_hash = chain_client.secp_type_hash()?;
        let address = get_address(m)?;
        address.lock_script(secp_type_hash).calc_script_hash()
    };

    Ok(lock_hash)
}
