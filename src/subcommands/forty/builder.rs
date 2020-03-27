use super::util::minimal_unlock_point;
use ckb_index::LiveCellInfo;
use ckb_sdk::{GenesisInfo, HttpRpcClient, Since, SinceType};
use ckb_types::core::Capacity;
use ckb_types::{
    bytes::Bytes,
    core::{HeaderView, ScriptHashType, TransactionBuilder, TransactionView},
    packed::{self, CellInput, CellOutput, OutPoint, Script, WitnessArgs},
    prelude::*,
};
use std::collections::HashSet;
use crate::subcommands::forty::command::IssueArgs;

// NOTE: We assume all inputs are from same account
#[derive(Debug)]
pub(crate) struct FortyBuilder {
    genesis_info: GenesisInfo,
    tx_fee: u64,
    live_cells: Vec<LiveCellInfo>,
}

impl FortyBuilder {
    pub(crate) fn new(
        genesis_info: GenesisInfo,
        tx_fee: u64,
        live_cells: Vec<LiveCellInfo>,
    ) -> Self {
        Self {
            genesis_info,
            tx_fee,
            live_cells,
        }
    }

    pub(crate) fn issue(&self, issue_args: &IssueArgs) -> Result<TransactionView, String> {
        let genesis_info = &self.genesis_info;
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
                .type_(Some(self.ft_type_script()).pack())
                .build();

            // OutputData format: [amount_hash, encrypted_amount]

            let output_data = Bytes::from(&[0u8; 8][..]).pack();
            (output, output_data)
        };
    }

    pub(crate) fn ft_type_script(&self) -> Script {
        // bilibili TODO
        unimplemented!()
    }
}
