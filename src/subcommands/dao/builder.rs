use crate::subcommands::functional::{can_prepare, ChainClient};
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

// NOTE: We assume all inputs from same account
#[derive(Debug)]
pub(crate) struct DAOBuilder {
    capacity: u64,
    tx_fee: u64,
    live_cells: Vec<LiveCellInfo>,
}

impl DAOBuilder {
    pub(crate) fn new(capacity: u64, tx_fee: u64, live_cells: Vec<LiveCellInfo>) -> Self {
        Self {
            capacity,
            tx_fee,
            live_cells,
        }
    }

    pub(crate) fn deposit(
        &self,
        chain_client: &mut ChainClient,
    ) -> Result<TransactionView, String> {
        let genesis_info = chain_client.genesis_info()?;
        let deposit_capacity = self.capacity;

        let inputs = self
            .live_cells
            .iter()
            .map(|txo| CellInput::new(txo.out_point(), 0))
            .collect::<Vec<_>>();
        let witnesses = inputs
            .iter()
            .map(|_| Default::default())
            .collect::<Vec<_>>();
        let (output, output_data) = {
            // NOTE: Here give null lock script to the output. It's caller's duty to fill the lock
            let output = CellOutput::new_builder()
                .capacity(deposit_capacity.pack())
                .type_(Some(dao_type_script(chain_client)?).pack())
                .build();
            let output_data = Bytes::from(&[0u8; 8][..]).pack();
            (output, output_data)
        };
        let cell_deps = vec![genesis_info.dao_dep()];
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .output(output)
            .output_data(output_data)
            .cell_deps(cell_deps)
            .witnesses(witnesses);

        let input_capacity = self.live_cells.iter().map(|txo| txo.capacity).sum::<u64>();
        let change_capacity = input_capacity - self.capacity - self.tx_fee;
        if change_capacity > 0 {
            let change = CellOutput::new_builder()
                .capacity(change_capacity.pack())
                .build();
            Ok(tx.output(change).output_data(Default::default()).build())
        } else {
            Ok(tx.build())
        }
    }

    pub(crate) fn prepare(
        &self,
        chain_client: &mut ChainClient,
    ) -> Result<TransactionView, String> {
        let genesis_info = chain_client.genesis_info()?;
        let mut deposit_cells: Vec<LiveCellInfo> = Vec::new();
        let mut change_cells: Vec<LiveCellInfo> = Vec::new();
        for info in self.live_cells.iter() {
            if can_prepare(chain_client, info)? {
                deposit_cells.push(info.clone());
            } else {
                change_cells.push(info.clone());
            }
        }
        let deposit_txo_headers = {
            let deposit_out_points = deposit_cells
                .iter()
                .map(|txo| txo.out_point())
                .collect::<Vec<_>>();
            self.txo_headers(chain_client, deposit_out_points)?
        };

        let inputs = self
            .live_cells
            .iter()
            .map(|txo| CellInput::new(txo.out_point(), 0))
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
        let witnesses = (0..inputs.len())
            .map(|_| WitnessArgs::default().as_bytes().pack())
            .collect::<Vec<_>>();
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .outputs(outputs)
            .cell_deps(cell_deps)
            .header_deps(header_deps)
            .witnesses(witnesses)
            .outputs_data(outputs_data);

        let change_capacity =
            change_cells.iter().map(|txo| txo.capacity).sum::<u64>() - self.tx_fee;
        let change = CellOutput::new_builder()
            .capacity(change_capacity.pack())
            .build();
        Ok(tx.output(change).output_data(Default::default()).build())
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

        let inputs = deposit_txo_headers
            .iter()
            .zip(prepare_txo_headers.iter())
            .map(|((_, _, deposit_header), (out_point, _, prepare_header))| {
                let minimal_unlock_point = minimal_unlock_point(deposit_header, prepare_header);
                let since = since_from_absolute_epoch_number(minimal_unlock_point.full_value());
                CellInput::new(out_point.clone(), since)
            });
        let total_capacity = deposit_txo_headers
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
        let output_capacity = total_capacity - self.tx_fee;
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
                    .ok_or_else(|| "Tx is not on-chain".to_owned())?;
                chain_client
                    .get_header(block_hash.pack())?
                    .expect("checked above")
            };

            let output_index: u32 = out_point.index().unpack();
            let output = tx
                .outputs()
                .get(output_index as usize)
                .ok_or_else(|| "OutPoint is out of index".to_owned())?;
            ret.push((out_point, output, header))
        }
        Ok(ret)
    }
}

pub fn minimal_unlock_point(
    deposit_header: &HeaderView,
    prepare_header: &HeaderView,
) -> EpochNumberWithFraction {
    const LOCK_PERIOD_EPOCHES: EpochNumber = 180;

    // https://github.com/nervosnetwork/ckb-system-scripts/blob/master/c/dao.c#L182-L223
    let deposit_point = deposit_header.epoch();
    let prepare_point = prepare_header.epoch();
    let prepare_fraction = prepare_point.index() * deposit_point.length();
    let deposit_fraction = deposit_point.index() * prepare_point.length();
    let passed_epoch_cnt = if prepare_fraction > deposit_fraction {
        prepare_point.number() - deposit_point.number() + 1
    } else {
        prepare_point.number() - deposit_point.number()
    };
    let rest_epoch_cnt =
        (passed_epoch_cnt + (LOCK_PERIOD_EPOCHES - 1)) / LOCK_PERIOD_EPOCHES * LOCK_PERIOD_EPOCHES;
    EpochNumberWithFraction::new(
        deposit_point.number() + rest_epoch_cnt,
        deposit_point.index(),
        deposit_point.length(),
    )
}

pub fn dao_type_script(chain_client: &mut ChainClient) -> Result<Script, String> {
    Ok(Script::new_builder()
        .hash_type(ScriptHashType::Type.into())
        .code_hash(chain_client.dao_type_hash()?)
        .build())
}

pub fn since_from_absolute_epoch_number(epoch_number: EpochNumber) -> u64 {
    const FLAG_SINCE_EPOCH_NUMBER: u64 =
        0b010_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;
    FLAG_SINCE_EPOCH_NUMBER | epoch_number
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_types::core::HeaderBuilder;

    #[test]
    fn test_minimal_unlock_point() {
        let cases = vec![
            ((5, 5, 1000), (184, 4, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (184, 5, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (184, 6, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (185, 4, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (185, 5, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (185, 6, 1000), (5 + 180 * 2, 5, 1000)), // 6/1000 > 5/1000
            ((5, 5, 1000), (186, 4, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (186, 5, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (186, 6, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (364, 4, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (364, 5, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (364, 6, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (365, 4, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (365, 5, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (365, 6, 1000), (5 + 180 * 3, 5, 1000)),
            ((5, 5, 1000), (366, 4, 1000), (5 + 180 * 3, 5, 1000)),
            ((5, 5, 1000), (366, 5, 1000), (5 + 180 * 3, 5, 1000)),
            ((5, 5, 1000), (366, 6, 1000), (5 + 180 * 3, 5, 1000)),
        ];
        for (deposit_point, prepare_point, expected) in cases {
            let deposit_point =
                EpochNumberWithFraction::new(deposit_point.0, deposit_point.1, deposit_point.2);
            let prepare_point =
                EpochNumberWithFraction::new(prepare_point.0, prepare_point.1, prepare_point.2);
            let expected = EpochNumberWithFraction::new(expected.0, expected.1, expected.2);
            let deposit_header = HeaderBuilder::default()
                .epoch(deposit_point.full_value().pack())
                .build();
            let prepare_header = HeaderBuilder::default()
                .epoch(prepare_point.full_value().pack())
                .build();
            let actual = minimal_unlock_point(&deposit_header, &prepare_header);
            assert_eq!(
                expected, actual,
                "minimal_unlock_point deposit_point: {}, prepare_point: {}, expected: {}, actual: {}",
                deposit_point, prepare_point, expected, actual,
            );
        }
    }
}
