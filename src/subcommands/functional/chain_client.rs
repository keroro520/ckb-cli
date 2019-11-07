use crate::utils::other::get_network_type;
use crate::utils::printer::{OutputFormat, Printable};
use ckb_index::LiveCellInfo;
use ckb_jsonrpc_types::{CellWithStatus, TransactionWithStatus};
use ckb_sdk::{GenesisInfo, HttpRpcClient, NetworkType};
use ckb_types::core::{BlockView, HeaderView, TransactionView};
use ckb_types::packed::{self, Byte32, OutPoint};
use ckb_types::prelude::*;

pub struct ChainClient<'a> {
    rpc_client: &'a mut HttpRpcClient,
    genesis_info: Option<GenesisInfo>,
}

impl<'a> ChainClient<'a> {
    pub fn new(rpc_client: &'a mut HttpRpcClient, genesis_info: Option<GenesisInfo>) -> Self {
        Self {
            rpc_client,
            genesis_info,
        }
    }

    pub fn rpc_client(&mut self) -> &mut HttpRpcClient {
        self.rpc_client
    }

    pub fn genesis_info(&mut self) -> Result<GenesisInfo, String> {
        self.ensure_genesis()?;
        Ok(self.genesis_info.clone().unwrap())
    }

    pub fn secp_type_hash(&mut self) -> Result<Byte32, String> {
        self.ensure_genesis()?;
        Ok(self.genesis_info.as_ref().unwrap().secp_type_hash().clone())
    }

    pub fn dao_type_hash(&mut self) -> Result<Byte32, String> {
        self.ensure_genesis()?;
        Ok(self.genesis_info.as_ref().unwrap().dao_type_hash().clone())
    }

    pub fn network_type(&mut self) -> Result<NetworkType, String> {
        get_network_type(self.rpc_client)
    }

    pub fn get_live_cell(&mut self, out_point: OutPoint) -> Result<CellWithStatus, String> {
        self.rpc_client
            .get_live_cell(out_point.into(), true)
            .call()
            .map_err(|err| format!("RPC get_live_cell error: {}", err))
    }

    pub fn get_transaction(&mut self, tx_hash: Byte32) -> Result<TransactionWithStatus, String> {
        self.rpc_client
            .get_transaction(tx_hash.unpack())
            .call()
            .map_err(|err| format!("RPC get_transaction error: {}", err))?
            .0
            .ok_or_else(|| "RPC get_transaction returns none".to_owned())
    }

    pub fn get_header(&mut self, block_hash: Byte32) -> Result<Option<HeaderView>, String> {
        self.rpc_client
            .get_header(block_hash.unpack())
            .call()
            .map_err(|err| format!("RPC get_header error: {}", err))
            .map(|h| h.0.map(Into::into))
    }

    pub fn send_transaction(
        &mut self,
        transaction: TransactionView,
        format: OutputFormat,
        debug: bool,
        color: bool,
    ) -> Result<String, String> {
        let transaction_view: ckb_jsonrpc_types::TransactionView = transaction.clone().into();
        if debug {
            println!(
                "[Send Transaction]:\n{}",
                transaction_view.render(format, color)
            );
        }

        let resp = self
            .rpc_client
            .send_transaction(transaction.data().into())
            .call()
            .map_err(|err| format!("Send transaction error: {}", err))?;
        Ok(resp.render(format, color))
    }

    pub fn calculate_dao_maximum_withdraw(&mut self, info: &LiveCellInfo) -> Result<u64, String> {
        let tx = self.get_transaction(info.tx_hash.pack())?;
        let prepare_block_hash = tx
            .tx_status
            .block_hash
            .ok_or("invalid prepare out_point, the tx is not committed")?;
        let tx: packed::Transaction = tx.transaction.inner.into();
        let tx = tx.into_view();

        let input = tx
            .inputs()
            .get(info.out_point().index().unpack())
            .expect("invalid prepare out_point");
        let deposit_out_point = input.previous_output();

        let maximum = self
            .rpc_client
            .calculate_dao_maximum_withdraw(deposit_out_point.into(), prepare_block_hash)
            .call()
            .map_err(|err| format!("RPC calculate_dao_maximum_withdraw failed: {:?}", err))?;
        Ok(maximum.value())
    }

    fn ensure_genesis(&mut self) -> Result<(), String> {
        if self.genesis_info.is_none() {
            let genesis_block: BlockView = self
                .rpc_client
                .get_block_by_number(0.into())
                .call()
                .map_err(|err| err.to_string())?
                .0
                .expect("Can not get genesis block?")
                .into();
            self.genesis_info = Some(GenesisInfo::from_block(&genesis_block)?);
        }
        Ok(())
    }
}
