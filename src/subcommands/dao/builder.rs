use crate::subcommands::functional::ChainClient;
use ckb_index::LiveCellInfo;
use ckb_types::core::{EpochNumber, EpochNumberWithFraction};
use ckb_types::prelude::Builder;
use ckb_types::{
    bytes::Bytes,
    core::{HeaderView, ScriptHashType, TransactionBuilder, TransactionView},
    packed::{self, CellInput, CellOutput, OutPoint, Script, WitnessArgs},
    prelude::*,
};
use std::collections::HashSet;

// TODO Allow tx_fee != 0
// NOTE: We assume all inputs from same account
#[derive(Debug)]
pub(crate) struct DAOBuilder {
    tx_fee: u64,
    live_cells: Vec<LiveCellInfo>,
}

impl DAOBuilder {
    pub(crate) fn new(tx_fee: u64, live_cells: Vec<LiveCellInfo>) -> Self {
        assert_eq!(0, tx_fee);
        Self { tx_fee, live_cells }
    }

    pub(crate) fn deposit(
        &self,
        chain_client: &mut ChainClient,
    ) -> Result<TransactionView, String> {
        let genesis_info = chain_client.genesis_info()?;
        let input_capacity = self.live_cells.iter().map(|txo| txo.capacity).sum::<u64>();
        let inputs = self
            .live_cells
            .iter()
            .map(|txo| CellInput::new(txo.out_point(), 0))
            .collect::<Vec<_>>();
        let witnesses = inputs
            .iter()
            .map(|_| Default::default())
            .collect::<Vec<_>>();
        let output = {
            // NOTE: Here give null lock script to the output. It's caller's duty to fill the lock
            CellOutput::new_builder()
                .capacity(input_capacity.pack())
                .type_(Some(dao_type_script(chain_client)?).pack())
                .build()
        };
        let output_data = Bytes::from(&[0u8; 8][..]).pack();
        let cell_deps = vec![genesis_info.dao_dep()];
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .output(output)
            .output_data(output_data)
            .cell_deps(cell_deps)
            .witnesses(witnesses)
            .build();
        Ok(tx)
    }

    pub(crate) fn prepare(
        &self,
        chain_client: &mut ChainClient,
    ) -> Result<TransactionView, String> {
        let genesis_info = chain_client.genesis_info()?;
        let deposit_txo_headers = {
            let deposit_out_points = self
                .live_cells
                .iter()
                .map(|txo| txo.out_point())
                .collect::<Vec<_>>();
            self.txo_headers(chain_client, deposit_out_points)?
        };

        let inputs = deposit_txo_headers
            .iter()
            .map(|(out_point, _, _)| CellInput::new(out_point.clone(), 0))
            .collect::<Vec<_>>();
        // NOTE: Prepare output has the same capacity, type script, lock script as the input
        let outputs = deposit_txo_headers
            .iter()
            .map(|(_, output, _)| output.clone())
            .collect::<Vec<_>>();
        let outputs_data = deposit_txo_headers.iter().map(|(_, _, header)| {
            let deposit_number = header.number();
            Bytes::from(deposit_number.to_le_bytes().to_vec()).pack()
        });
        let cell_deps = vec![genesis_info.dao_dep()];
        let header_deps = deposit_txo_headers
            .iter()
            .map(|(_, _, header)| header.hash())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let witnesses = deposit_txo_headers
            .iter()
            .map(|(_, _, header)| {
                let index = header_deps
                    .iter()
                    .position(|hash| hash == &header.hash())
                    .unwrap() as u64;
                WitnessArgs::new_builder()
                    .input_type(Some(Bytes::from(index.to_le_bytes().to_vec())).pack())
                    .build()
                    .as_bytes()
                    .pack()
            })
            .collect::<Vec<_>>();
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .outputs(outputs)
            .cell_deps(cell_deps)
            .header_deps(header_deps)
            .witnesses(witnesses)
            .outputs_data(outputs_data)
            .build();
        Ok(tx)
    }

    pub(crate) fn withdraw(
        &self,
        chain_client: &mut ChainClient,
    ) -> Result<TransactionView, String> {
        let genesis_info = chain_client.genesis_info()?;
        let prepare_txo_headers = {
            let prepare_out_points = self
                .live_cells
                .iter()
                .map(|txo| txo.out_point())
                .collect::<Vec<_>>();
            self.txo_headers(chain_client, prepare_out_points)?
        };
        let deposit_txo_headers = {
            let deposit_out_points = prepare_txo_headers
                .iter()
                .map(|(out_point, _, _)| {
                    let tx: packed::Transaction = chain_client
                        .get_transaction(out_point.tx_hash())
                        .expect("checked above")
                        .transaction
                        .inner
                        .into();
                    let tx = tx.into_view();
                    let input = tx
                        .inputs()
                        .get(out_point.index().unpack())
                        .expect("prepare out_point has the same index with deposit input");
                    input.previous_output()
                })
                .collect::<Vec<_>>();
            self.txo_headers(chain_client, deposit_out_points)?
        };

        let inputs = prepare_txo_headers.iter().map(|(out_point, _, header)| {
            let minimal_unlock_point = self.minimal_unlock_point(header);
            let since = since_from_absolute_epoch_number(minimal_unlock_point.full_value());
            CellInput::new(out_point.clone(), since)
        });
        let output_capacity = deposit_txo_headers
            .iter()
            .zip(prepare_txo_headers.iter())
            .map(|((deposit_txo, _, _), (_, _, prepare_header))| {
                chain_client
                    .rpc_client()
                    .calculate_dao_maximum_withdraw(
                        deposit_txo.clone().into(),
                        prepare_header.hash().unpack(),
                    )
                    .call()
                    .expect("RPC calculate_dao_maximum_withdraw failed")
                    .value()
            })
            .sum::<u64>();
        let output = CellOutput::new_builder()
            .capacity(output_capacity.pack())
            .build();
        let cell_deps = vec![genesis_info.dao_dep()];
        let header_deps = deposit_txo_headers
            .iter()
            .chain(prepare_txo_headers.iter())
            .map(|(_, _, header)| header.hash())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let witnesses = deposit_txo_headers
            .iter()
            .map(|(_, _, header)| {
                let index = header_deps
                    .iter()
                    .position(|hash| hash == &header.hash())
                    .unwrap() as u64;
                WitnessArgs::new_builder()
                    .input_type(Some(Bytes::from(index.to_le_bytes().to_vec())).pack())
                    .build()
                    .as_bytes()
                    .pack()
            })
            .collect::<Vec<_>>();
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .output(output)
            .cell_deps(cell_deps)
            .header_deps(header_deps)
            .witnesses(witnesses)
            .output_data(Default::default())
            .build();
        Ok(tx)
    }

    fn txo_headers(
        &self,
        chain_client: &mut ChainClient,
        out_points: Vec<OutPoint>,
    ) -> Result<Vec<(OutPoint, CellOutput, HeaderView)>, String> {
        let mut ret = Vec::new();
        for out_point in out_points.into_iter() {
            let tx_status = chain_client.get_transaction(out_point.tx_hash())?;
            let tx: packed::Transaction = tx_status.transaction.inner.into();
            let tx = tx.into_view();
            let header = {
                let block_hash = tx_status
                    .tx_status
                    .block_hash
                    .ok_or("Tx is not on-chain".to_owned())?;
                chain_client
                    .get_header(block_hash.pack())?
                    .expect("checked above")
            };

            let output_index: u32 = out_point.index().unpack();
            let output = tx
                .outputs()
                .get(output_index as usize)
                .ok_or("OutPoint is out of index".to_owned())?;
            ret.push((out_point, output, header))
        }
        Ok(ret)
    }

    fn minimal_unlock_point(&self, deposit_header: &HeaderView) -> EpochNumberWithFraction {
        const LOCK_PERIOD_EPOCHES: EpochNumber = 180;
        let deposit_point = deposit_header.epoch();
        EpochNumberWithFraction::new(
            deposit_point.number() + LOCK_PERIOD_EPOCHES,
            deposit_point.index(),
            deposit_point.length(),
        )
    }
}

fn dao_type_script(chain_client: &mut ChainClient) -> Result<Script, String> {
    Ok(Script::new_builder()
        .hash_type(ScriptHashType::Type.into())
        .code_hash(chain_client.dao_type_hash()?)
        .build())
}

fn since_from_absolute_epoch_number(epoch_number: EpochNumber) -> u64 {
    const FLAG_SINCE_EPOCH_NUMBER: u64 =
        0b010_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;
    FLAG_SINCE_EPOCH_NUMBER | epoch_number
}
