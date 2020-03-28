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
use ckb_types::packed::{CellDep, Byte32, Bytes, BytesOpt};
use ckb_sdk::constants::MIN_SECP_CELL_CAPACITY;

pub struct ZKProof {
    // TODO
}

// NOTE: We assume all inputs are from same account
#[derive(Debug)]
pub struct FortyBuilder {
    genesis_info: GenesisInfo,
    tx_fee: u64,
    live_cells: Vec<LiveCellInfo>,
}

impl FortyBuilder {
    pub fn new(
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
    pub fn issue(&self, issue_args: &IssueArgs) -> Result<TransactionView, String> {
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
        let ft_type_script = issue_args.ft_type_script();
        let (output, output_data) = {
            // OutputData Format: [amount_hash, encrypted_amount]
            let output_data = issue_args.ft_output_data();

            // NOTE: Here give null lock script to the output. It's caller's duty to fill the lock
            let output = CellOutput::new_builder()
                .type_(Some(ft_type_script).pack())
                .build_exact_capacity(
                    Capacity::bytes(output_data.len()).unwrap()
                ).expect("build issued FT output");
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
    }

    // NOTE: Only support 1 output by now
    pub fn transfer(&self, transact_args: &TransactArgs, proof: Bytes) -> Result<TransactionView, String> {
        // let genesis_info = &self.genesis_info;
        let inputs = self
            .live_cells
            .iter()
            .map(|txo| CellInput::new(txo.out_point(), 0))
            .collect::<Vec<_>>();
        // NOTE: As for transfer, Witness.output_type holds zk-proof
        let witnesses = inputs
            .iter()
            .map(|_| {
                let output_type_witness = BytesOpt::new_builder().set(Some(proof.clone())).build();
                WitnessArgs::new_builder()
                    .output_type(output_type_witness)
                    .build()
                    .as_bytes()
            })
            .collect::<Vec<_>>();
        let ft_type_script = transact_args.ft_type_script();
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
                occupied.as_u64() + self.tx_fee <= input_capacity,
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
            .witnesses(witnesses.into_iter().map(|w| w.pack()).collect::<Vec<_>>());

        // Handle the CKB change problem (Reiterate, it's not FT Token change)
        //
        // No! We will not handle the CKB change problem here! Assume we have enough
        // CKB by now.

        Ok(tx.build())
    }
}
