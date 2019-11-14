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
use ckb_index::LiveCellInfo;
use ckb_sdk::{MIN_SECP_CELL_CAPACITY, Address, AddressPayload, NetworkType, SECP256K1};
use ckb_types::core::TransactionView;
use ckb_types::packed::{Byte32, CellOutput, Script};
use ckb_types::prelude::*;
use ckb_types::{H160, H256};
use clap::ArgMatches;
use itertools::Itertools;
use std::collections::HashSet;

mod builder;
mod command;

pub(crate) struct TransactArgs {
    pub(crate) privkey: Option<PrivkeyWrapper>,
    pub(crate) account: Option<H160>,
    pub(crate) address: Address,
    pub(crate) with_password: bool,
    pub(crate) capacity: u64,
    pub(crate) tx_fee: u64,
}

impl TransactArgs {
    pub(crate) fn from_matches(m: &ArgMatches, network_type: NetworkType) -> Result<Self, String> {
        let privkey: Option<PrivkeyWrapper> =
            PrivkeyPathParser.from_matches_opt(m, "privkey-path", false)?;
        let account: Option<H160> =
            FixedHashParser::<H160>::default().from_matches_opt(m, "from-account", false)?;
        let address = if let Some(privkey) = privkey.as_ref() {
            let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, privkey);
            let payload = AddressPayload::from_pubkey(&pubkey);
            Address::new(network_type, payload)
        } else {
            let payload = AddressPayload::from_pubkey_hash(account.clone().unwrap());
            Address::new(network_type, payload)
        };
        let capacity: u64 = CapacityParser.from_matches(m, "capacity")?;
        let tx_fee: u64 = CapacityParser.from_matches(m, "tx-fee")?;
        let with_password = m.is_present("with-password");
        Ok(Self {
            privkey,
            account,
            address,
            with_password,
            capacity,
            tx_fee,
        })
    }
}

pub struct DAOSubCommand<'a> {
    chain_client: &'a mut ChainClient<'a>,
    index_client: &'a mut IndexClient<'a>,
    output_style: (OutputFormat, bool, bool),
    transact_args: Option<TransactArgs>,
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
            transact_args: None,
        }
    }

    pub fn deposit(
        &mut self,
        m: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        let network_type = self.chain_client.network_type()?;
        self.output_style = (format, color, debug);
        self.transact_args = Some(TransactArgs::from_matches(m, network_type)?);
        self.check_db_ready()?;

        let target_capacity = self.transact_args().capacity + self.transact_args().tx_fee;
        let infos = self.collect_secp_cells(target_capacity)?;
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
        self.transact_args = Some(TransactArgs::from_matches(m)?);
        self.check_db_ready()?;
        let target_capacity = self.transact_args().capacity;
        let tx_fee = self.transact_args().tx_fee;
        let mut infos = self.collect_deposit_cells(target_capacity)?;
        infos.extend(self.collect_secp_cells(tx_fee)?.into_iter());

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
        self.transact_args = Some(TransactArgs::from_matches(m)?);
        self.check_db_ready()?;

        let target_capacity = self.transact_args().capacity + self.transact_args().tx_fee;
        let infos = self.collect_prepare_cells(target_capacity)?;
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
        let lock_hash = query_args(m, self.chain_client)?;
        let infos = self.query_cells(lock_hash)?;
        let infos = infos
            .into_iter()
            .filter(|info| can_prepare(self.chain_client, info).unwrap_or(false))
            .collect::<Vec<_>>();
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

        let infos: Vec<LiveCellInfo> = {
            let lock_hash = query_args(m, self.chain_client)?;
            let infos = self.query_cells(lock_hash)?;
            infos
                .into_iter()
                .filter(|info| can_withdraw(self.chain_client, info).unwrap_or(false))
                .collect()
        };
        let total_capacity = infos.iter().map(|live| live.capacity).sum::<u64>();

        let mut total_maximum_withdraw = 0;
        let infos_with_interest = infos.into_iter().map(|info| {
            let maximum_withdraw = self
                .chain_client
                .calculate_dao_maximum_withdraw(&info)
                .expect("calculate_dao_maximum_withdraw failed; TODO");
            total_maximum_withdraw += maximum_withdraw;
            (info, maximum_withdraw)
        });

        let resp = serde_json::json!({
            "live_cells": infos_with_interest.map(|(info, maximum_withdraw)| {
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

    fn query_cells(&mut self, lock_hash: Byte32) -> Result<Vec<LiveCellInfo>, String> {
        let genesis_info = self.chain_client.genesis_info()?;
        let network_type = self.chain_client.network_type()?;
        let dao_type_hash = self.chain_client.dao_type_hash()?;
        self.index_client.with_db(network_type, genesis_info, |db| {
            let infos_by_lock = db
                .get_live_cells_by_lock(lock_hash, Some(0), |_, _| (false, true))
                .into_iter()
                .collect::<HashSet<_>>();
            let infos_by_code = db
                .get_live_cells_by_code(dao_type_hash, Some(0), |_, _| (false, true))
                .into_iter()
                .collect::<HashSet<_>>();
            infos_by_lock
                .intersection(&infos_by_code)
                .sorted_by_key(|live| (live.number, live.tx_index, live.index.output_index))
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    fn build(&self, infos: Vec<LiveCellInfo>) -> DAOBuilder {
        let capacity = self.transact_args().capacity;
        let tx_fee = self.transact_args().tx_fee;
        DAOBuilder::new(capacity, tx_fee, infos)
    }

    fn sign(&mut self, transaction: TransactionView) -> Result<TransactionView, String> {
        let transact_args = &self.transact_args;
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;

        let from_privkey = &transact_args.as_ref().unwrap().privkey;
        let from_account = &transact_args.as_ref().unwrap().account;
        let from_address = &transact_args.as_ref().unwrap().address;
        let with_password = transact_args.as_ref().unwrap().with_password;
        let secp_cell_dep = chain_client.genesis_info()?.sighash_dep();
        let interactive = index_client.interactive();

        // Build outputs' SECP locks, and remember putting SECP cell into cell_deps
        let outputs = transaction
            .outputs()
            .into_iter()
            .map(|output: CellOutput| {
                output
                    .as_builder()
                    .lock(from_address.payload().into())
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
        let witnesses = if let Some(ref privkey) = from_privkey {
            build_secp_witnesses(tx_hash, witnesses, |digest| {
                sign_with_privkey(privkey, digest)
            })?
        } else {
            let lock_arg = from_account.as_ref().unwrap();
            let password = if with_password {
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

    fn collect_secp_cells(&mut self, target_capacity: u64) -> Result<Vec<LiveCellInfo>, String> {
        let genesis_info = self.chain_client.genesis_info()?;
        let secp_type_hash = self.chain_client.secp_type_hash()?;
        let network_type = self.chain_client.network_type()?;
        let from_address = self.transact_args().address.clone();
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let mut enough = false;
        let mut take_capacity = 0;
        let terminator = |_, info: &LiveCellInfo| {
            if Ok(true) != can_deposit(chain_client, info) {
                return (false, false);
            }

            take_capacity += info.capacity;
            if take_capacity == target_capacity
                || take_capacity >= target_capacity + *MIN_SECP_CELL_CAPACITY
            {
                enough = true;
            }
            (enough, true)
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
                from_address.display_with_prefix(network_type),
                take_capacity,
            ));
        }
        Ok(infos)
    }

    fn collect_deposit_cells(&mut self, target_capacity: u64) -> Result<Vec<LiveCellInfo>, String> {
        let genesis_info = self.chain_client.genesis_info()?;
        let secp_type_hash = self.chain_client.secp_type_hash()?;
        let network_type = self.chain_client.network_type()?;
        let from_address = self.transact_args().address.clone();
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let mut enough = false;
        let mut take_capacity = 0;
        let terminator = |_, info: &LiveCellInfo| {
            if Ok(true) != can_prepare(chain_client, info) {
                return (false, false);
            }

            if info.capacity == target_capacity {
                take_capacity = info.capacity;
                enough = true;
                (true, true)
            } else {
                (false, false)
            }
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
                from_address.display_with_prefix(network_type),
                take_capacity,
            ));
        }
        Ok(infos)
    }

    fn collect_prepare_cells(&mut self, target_capacity: u64) -> Result<Vec<LiveCellInfo>, String> {
        let genesis_info = self.chain_client.genesis_info()?;
        let secp_type_hash = self.chain_client.secp_type_hash()?;
        let network_type = self.chain_client.network_type()?;
        let from_address = self.transact_args().address.clone();
        let chain_client = &mut self.chain_client;
        let index_client = &mut self.index_client;
        let mut enough = false;
        let mut take_capacity = 0;
        let terminator = |_, info: &LiveCellInfo| {
            if Ok(true) != can_withdraw(chain_client, info) {
                return (false, false);
            }

            let max_withdrawal: u64 = chain_client
                .calculate_dao_maximum_withdraw(&info)
                .expect("RPC calculate_dao_maximum_withdraw for a prepare cell");
            if max_withdrawal == target_capacity {
                take_capacity = max_withdrawal;
                enough = true;
                (true, true)
            } else {
                (false, false)
            }
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
                from_address.display_with_prefix(network_type),
                take_capacity,
            ));
        }
        Ok(infos)
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

    fn transact_args(&self) -> &TransactArgs {
        self.transact_args.as_ref().expect("exist")
    }
}

fn query_args(m: &ArgMatches, chain_client: &mut ChainClient) -> Result<Byte32, String> {
    let lock_hash_opt: Option<H256> =
        FixedHashParser::<H256>::default().from_matches_opt(m, "lock-hash", false)?;
    let lock_hash = if let Some(lock_hash) = lock_hash_opt {
        lock_hash.pack()
    } else {
        let network_type = chain_client.network_type()?;
        let address = get_address(Some(network_type), m)?;
        Script::from(&address).calc_script_hash()
    };

    Ok(lock_hash)
}
