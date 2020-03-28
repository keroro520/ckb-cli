use ckb_index::LiveCellInfo;
use ckb_sdk::{GenesisInfo, HttpRpcClient, Since, SinceType};
use ckb_types::core::Capacity;
use ckb_types::{
    core::{HeaderView, ScriptHashType, TransactionBuilder, TransactionView},
    packed::{self, CellInput, CellOutput, OutPoint, Script, WitnessArgs},
    prelude::*,
};
use std::collections::HashSet;
use crate::subcommands::forty::command::{IssueArgs, TransactArgs};
use ckb_types::packed::{CellDep, Byte32, Bytes};
use ckb_sdk::constants::MIN_SECP_CELL_CAPACITY;

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

    // NOTE: Only support 1 output by now
    pub(crate) fn issue(&self, issue_args: &IssueArgs) -> Result<TransactionView, String> {
        // let genesis_info = &self.genesis_info;
        let inputs = self
            .live_cells
            .iter()
            .map(|txo| CellInput::new(txo.out_point(), 0))
            .collect::<Vec<_>>();
        let witnesses = inputs
            .iter()
            .map(|_| Default::default())
            .collect::<Vec<_>>();
        let ft_type_script = {
            let ft_code_hash = issue_args.ft_code_hash();
            let ft_lock_args = issue_args.ft_lock_args();
            Script::new_builder()
                .hash_type(ScriptHashType::Data.into())
                .code_hash(ft_code_hash)
                .args(ft_lock_args)
                .build()
        };
        let (output, output_data) = {
            // OutputData Format: [amount_hash, encrypted_amount]
            let output_data = issue_args.ft_output_data();

            // NOTE: Here give null lock script to the output. It's caller's duty to fill the lock
            let output = CellOutput::new_builder()
                .type_(Some(ft_type_script).pack())
                .build_exact_capacity(
                    Capacity::bytes(output_data.len()).unwrap().unwrap()
                )
                .build();
            (output, output_data)
        };
        let cell_deps = vec![issue_args.ft_cell_dep()];
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .output(output.clone())
            .output_data(output_data)
            .cell_deps(cell_deps)
            .witnesses(witnesses);

        // Handle CKB change problem (Reiterate, it's not FT Token change)
        let output_capacity: u64 = output.capacity().unpack();
        let input_capacity = self.live_cells.iter().map(|txo| txo.capacity).sum::<u64>();
        assert!(
            input_capacity > output_capacity + self.tx_fee,
            "Must ensure input_capacity > output_capacity + tx_fee",
        );

        let change_capacity = input_capacity - output_capacity - self.tx_fee;
        if change_capacity >= MIN_SECP_CELL_CAPACITY {
            let change = CellOutput::new_builder()
                .capacity(change_capacity.pack())
                .build();
            Ok(tx.output(change).output_data(Default::default()).build())
        } else {
            Ok(tx.build())
        }
        Ok(tx.build())
    }

    // NOTE: Only support 1 output by now
    pub(crate) fn transfer(&self, transact_args: &TransactArgs) -> Result<TransactionView, String> {
        // let genesis_info = &self.genesis_info;
        let inputs = self
            .live_cells
            .iter()
            .map(|txo| CellInput::new(txo.out_point(), 0))
            .collect::<Vec<_>>();
        // TODO witness 要改
        let witnesses = inputs
            .iter()
            .map(|_| Default::default())
            .collect::<Vec<_>>();
        let ft_type_script = {
            let ft_code_hash = transact_args.ft_code_hash();
            let ft_lock_args = transact_args.ft_lock_args();
            Script::new_builder()
                .hash_type(ScriptHashType::Data.into())
                .code_hash(ft_code_hash)
                .args(ft_lock_args)
                .build()
        };
        let (output, output_data) = {
            // OutputData Format: [amount_hash, encrypted_amount]
            let output_data = transact_args.ft_output_data();

            // NOTE: Here give null lock script to the output. It's caller's duty to fill the lock
            let input_capacity = self.live_cells.iter().map(|txo| txo.capacity).sum::<u64>();
            let output = CellOutput::new_builder()
                .capacity(input_capacity.pack())
                .type_(Some(ft_type_script).pack())
                .build();

            let occupied = output.occupied_capacity(
                Capacity::bytes(output_data.len()).unwrap()
            ).expect("output.occupied_capacity()");
            assert!(
                occupied.as_u64() + self.tx_fee <= input_capacity
                "output.occupied_capacity() + tx_fee > input.capacity()",
            );

            (output, output_data)
        };
        let cell_deps = vec![transact_args.ft_cell_dep()];
        let tx = TransactionBuilder::default()
            .inputs(inputs)
            .output(output.clone())
            .output_data(output_data)
            .cell_deps(cell_deps)
            .witnesses(witnesses);

        // Handle the CKB change problem (Reiterate, it's not FT Token change)
        //
        // No! We will not handle the CKB change problem here! Assume we have enough
        // CKB by now.

        Ok(tx.build())
    }
}
