use crate::utils::{
    other::check_lack_of_capacity,
    printer::{OutputFormat, Printable},
};
use ckb_index::LiveCellInfo;
use ckb_sdk::HttpRpcClient;
use ckb_types::core::{Capacity, TransactionView};
use ckb_types::packed::CellOutput;
use ckb_types::{
    core::{EpochNumber, EpochNumberWithFraction, HeaderView},
    packed,
    prelude::*,
};


pub(crate) fn send_transaction(
    rpc_client: &mut HttpRpcClient,
    transaction: TransactionView,
    format: OutputFormat,
    color: bool,
    debug: bool,
) -> Result<String, String> {
    check_lack_of_capacity(&transaction)?;
    let transaction_view: ckb_jsonrpc_types::TransactionView = transaction.clone().into();
    if debug {
        println!(
            "[Send Transaction]:\n{}",
            transaction_view.render(format, color)
        );
    }

    let resp = rpc_client.send_transaction(transaction.data())?;
    Ok(resp.render(format, color))
}
