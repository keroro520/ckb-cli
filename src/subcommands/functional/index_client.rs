use crate::subcommands::IndexController;
use ckb_index::{with_index_db, IndexDatabase};
use ckb_sdk::wallet::KeyStore;
use ckb_sdk::{GenesisInfo, NetworkType};
use ckb_types::prelude::*;
use ckb_types::H256;
use std::path::PathBuf;

pub struct IndexClient<'a> {
    key_store: &'a mut KeyStore,
    index_dir: PathBuf,
    index_controller: IndexController,
    interactive: bool,
}

impl<'a> IndexClient<'a> {
    pub fn new(
        key_store: &'a mut KeyStore,
        index_dir: PathBuf,
        index_controller: IndexController,
        interactive: bool,
    ) -> Self {
        Self {
            key_store,
            index_dir,
            index_controller,
            interactive,
        }
    }

    pub fn with_db<F, T>(
        &mut self,
        network_type: NetworkType,
        genesis_info: GenesisInfo,
        func: F,
    ) -> Result<T, String>
    where
        F: FnOnce(IndexDatabase) -> T,
    {
        if !self.interactive {
            return Err("ERROR: This is an interactive mode only sub-command".to_string());
        }

        let genesis_hash: H256 = genesis_info.header().hash().unpack();
        with_index_db(&self.index_dir, genesis_hash, |backend, cf| {
            let db = IndexDatabase::from_db(backend, cf, network_type, genesis_info, false)?;
            Ok(func(db))
        })
        .map_err(|_err| {
            format!(
                "index database may not ready, sync process: {}",
                self.state(),
            )
        })
    }

    pub fn key_store(&mut self) -> &mut KeyStore {
        self.key_store
    }

    pub fn interactive(&self) -> bool {
        self.interactive
    }

    pub fn state(&self) -> String {
        self.index_controller.state().read().to_string()
    }
}
