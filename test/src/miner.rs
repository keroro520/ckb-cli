use ckb_sdk::HttpRpcClient;
use ckb_types::packed::Block;
use ckb_types::H256;

pub struct Miner {
    rpc: HttpRpcClient,
}

impl Miner {
    pub fn init(uri: &str) -> Self {
        Self {
            rpc: HttpRpcClient::from_uri(uri),
        }
    }

    pub fn generate_block(&mut self) -> H256 {
        let template = self
            .rpc
            .get_block_template(None, None, None)
            .call()
            .expect("RPC get_block_template");
        let work_id = template.work_id.value();
        let block = Into::<Block>::into(template);
        self.rpc
            .submit_block(work_id.to_string(), block.into())
            .call()
            .expect("RPC submit_block")
    }

    pub fn generate_blocks(&mut self, count: u64) {
        (0..count).for_each(|_| {
            self.generate_block();
        })
    }
}
