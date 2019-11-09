use crate::subcommands::functional::ChainClient;
use byteorder::{ByteOrder, LittleEndian};
use ckb_index::LiveCellInfo;
use ckb_jsonrpc_types::{CellWithStatus, ScriptHashType};
use ckb_types::packed::Byte32;
use ckb_types::prelude::*;

pub fn can_spend(chain_client: &mut ChainClient, info: &LiveCellInfo) -> Result<bool, String> {
    let secp_type_hash = chain_client.secp_type_hash()?;
    let cell = chain_client.get_live_cell(info.out_point())?;
    Ok(is_live_secp_cell(&cell, &secp_type_hash)
        && is_none_type_cell(&cell)
        && is_empty_data(&cell))
}

pub fn can_deposit(chain_client: &mut ChainClient, info: &LiveCellInfo) -> Result<bool, String> {
    can_spend(chain_client, info)
}

pub fn can_prepare(chain_client: &mut ChainClient, info: &LiveCellInfo) -> Result<bool, String> {
    let secp_type_hash = chain_client.secp_type_hash()?;
    let dao_type_hash = chain_client.dao_type_hash()?;
    let cell = chain_client.get_live_cell(info.out_point())?;
    Ok(is_live_secp_cell(&cell, &secp_type_hash) && is_live_deposit_cell(&cell, &dao_type_hash))
}

pub fn can_withdraw(chain_client: &mut ChainClient, info: &LiveCellInfo) -> Result<bool, String> {
    let secp_type_hash = chain_client.secp_type_hash()?;
    let dao_type_hash = chain_client.dao_type_hash()?;
    let cell = chain_client.get_live_cell(info.out_point())?;
    Ok(is_live_secp_cell(&cell, &secp_type_hash) && is_live_prepare_cell(&cell, &dao_type_hash))
}

pub fn is_live_cell(cell: &CellWithStatus) -> bool {
    if cell.status != "live" {
        eprintln!(
            "[ERROR]: Not live cell({:?}) status: {}",
            cell.cell.as_ref().map(|info| &info.output),
            cell.status
        );
        return false;
    }

    if cell.cell.is_none() {
        eprintln!(
            "[ERROR]: No output found for cell: {:?}",
            cell.cell.as_ref().map(|info| &info.output)
        );
        return false;
    }

    true
}

pub fn is_none_type_cell(cell: &CellWithStatus) -> bool {
    cell.cell
        .as_ref()
        .map(|cell| cell.output.type_.is_none())
        .unwrap_or(true)
}

pub fn is_empty_data(cell: &CellWithStatus) -> bool {
    cell.cell
        .as_ref()
        .map(|cell| cell.data.as_ref().unwrap().content.is_empty())
        .unwrap_or(false)
}

pub fn is_live_secp_cell(cell: &CellWithStatus, secp_type_hash: &Byte32) -> bool {
    if !is_live_cell(cell) {
        return false;
    }

    let cell = cell.cell.as_ref().expect("checked above");
    if &cell.output.lock.code_hash.pack() != secp_type_hash {
        eprintln!("[ERROR]: No locked by SECP lock script: {:?}", cell,);
        return false;
    }

    if cell.output.lock.hash_type != ScriptHashType::Type {
        eprintln!(
            "[ERROR]: Locked by SECP lock script but not ScriptHashType::Type: {:?}",
            cell,
        );
        return false;
    }

    true
}

pub fn is_live_deposit_cell(cell: &CellWithStatus, dao_type_hash: &Byte32) -> bool {
    if !is_live_cell(cell) {
        return false;
    }

    let cell = cell.cell.as_ref().expect("checked above");
    let content = cell
        .data
        .as_ref()
        .map(|cell_data| cell_data.content.clone().into_bytes().pack())
        .unwrap();
    if content.len() != 8 {
        return false;
    }

    if LittleEndian::read_u64(&content.raw_data()[0..8]) != 0 {
        return false;
    }

    cell.output
        .type_
        .as_ref()
        .map(|script| {
            script.hash_type == ScriptHashType::Type && &script.code_hash.pack() == dao_type_hash
        })
        .unwrap_or(false)
}

pub fn is_live_prepare_cell(cell: &CellWithStatus, dao_type_hash: &Byte32) -> bool {
    if !is_live_cell(cell) {
        return false;
    }

    let cell = cell.cell.as_ref().expect("checked above");
    let content = cell
        .data
        .as_ref()
        .map(|cell_data| cell_data.content.clone().into_bytes().pack())
        .unwrap();
    if content.len() != 8 {
        return false;
    }

    let deposited_number = LittleEndian::read_u64(&content.raw_data()[0..8]);
    if deposited_number == 0 {
        return false;
    }

    cell.output
        .type_
        .as_ref()
        .map(|script| {
            script.hash_type == ScriptHashType::Type && &script.code_hash.pack() == dao_type_hash
        })
        .unwrap_or(false)
}
