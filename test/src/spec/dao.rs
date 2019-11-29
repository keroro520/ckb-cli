use crate::miner::Miner;
use crate::spec::{Setup, Spec};
use ckb_chain_spec::ChainSpec;
use ckb_sdk::NetworkType;
use ckb_types::core::EpochNumber;

const EPOCH_LENGTH: u64 = 1;
const LOCK_PERIOD_EPOCHES: EpochNumber = 180;

pub struct PrepareInFirstPeriod;

impl Spec for PrepareInFirstPeriod {
    fn run(&self, setup: &Setup) {
        let mut miner = Miner::init(&setup.rpc_url());
        let consensus = setup.consensus();
        let closest = consensus.tx_proposal_window().closest();
        let farthest = consensus.tx_proposal_window().farthest();
        miner.generate_blocks(farthest + 3);

        // Deposit
        let output = deposit(setup, 1103.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);
        let output = deposit(setup, 102.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);

        // Prepare
        query_deposit_live_cells(setup);
        let output = prepare(setup, 1103.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);
        let output = prepare(setup, 102.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);

        // Withdraw
        miner.generate_blocks(closest + 1 + LOCK_PERIOD_EPOCHES);
        query_prepare_live_cells(setup);
        let output = withdraw(setup, 1156.0, 0.618_247_71);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);
        let output = withdraw(setup, 102.0, 0.000_000_01);
        assert!(output.contains("Capacity not enough"), "{}", output);
        let output = withdraw(setup, 101.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
    }

    fn modify_spec_toml(&self, spec_toml: &mut ChainSpec) {
        spec_toml.params.genesis_epoch_length = EPOCH_LENGTH;
        spec_toml.params.permanent_difficulty_in_dummy = true;
    }
}

pub struct PrepareAtEndOfFirstPeriod;

impl Spec for PrepareAtEndOfFirstPeriod {
    fn run(&self, setup: &Setup) {
        let mut miner = Miner::init(&setup.rpc_url());
        let consensus = setup.consensus();
        let closest = consensus.tx_proposal_window().closest();
        let farthest = consensus.tx_proposal_window().farthest();
        miner.generate_blocks(farthest + 3);

        // Deposit
        let output = deposit(setup, 1103.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);
        let deposit_number = farthest + 3 + closest + 1;
        assert_eq!(
            format!("{:#x}", deposit_number),
            setup.cli("rpc get_tip_block_number"),
        );

        // Prepare at the end of the first lock-period
        miner.generate_blocks(LOCK_PERIOD_EPOCHES - (closest + 1));
        let output = prepare(setup, 1103.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);
        assert_eq!(
            format!("{:#x}", deposit_number + LOCK_PERIOD_EPOCHES),
            setup.cli("rpc get_tip_block_number"),
        );

        // Withdraw
        query_prepare_live_cells(setup);
        let output = withdraw(setup, 1769.0, 0.362_447_81);
        assert_eq!(output.len(), 66, "{}", output);
    }

    fn modify_spec_toml(&self, spec_toml: &mut ChainSpec) {
        spec_toml.params.genesis_epoch_length = EPOCH_LENGTH;
        spec_toml.params.permanent_difficulty_in_dummy = true;
    }
}

pub struct PrepareInSecondPeriod;

impl Spec for PrepareInSecondPeriod {
    fn run(&self, setup: &Setup) {
        let mut miner = Miner::init(&setup.rpc_url());
        let consensus = setup.consensus();
        let closest = consensus.tx_proposal_window().closest();
        let farthest = consensus.tx_proposal_window().farthest();
        miner.generate_blocks(farthest + 3);

        // Deposit
        let output = deposit(setup, 1103.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);
        let deposit_number = farthest + 3 + closest + 1;
        assert_eq!(
            format!("{:#x}", deposit_number),
            setup.cli("rpc get_tip_block_number"),
        );

        // Drive into the second lock-period
        miner.generate_blocks(LOCK_PERIOD_EPOCHES + closest + 1);

        // Prepare at the end of the first lock-period
        let output = prepare(setup, 1103.0, 1.0);
        assert_eq!(output.len(), 66, "{}", output);
        miner.generate_blocks(closest + 1);

        // Withdraw, should fail by immature
        query_prepare_live_cells(setup);
        let output = withdraw(setup, 1781.0, 0.065_078_39);
        assert!(output.contains("Immature"), "{}", output);

        miner.generate_blocks(deposit_number + 2 * LOCK_PERIOD_EPOCHES - (closest + 1));
        let output = withdraw(setup, 1781.0, 0.065_078_39);
        assert_eq!(output.len(), 66, "{}", output);
    }

    fn modify_spec_toml(&self, spec_toml: &mut ChainSpec) {
        spec_toml.params.genesis_epoch_length = EPOCH_LENGTH;
        spec_toml.params.permanent_difficulty_in_dummy = true;
    }
}

fn deposit(setup: &Setup, amount: f64, tx_fee: f64) -> String {
    let output = setup.cli(&format!(
        "dao deposit --privkey-path {} --capacity {} --tx-fee {}",
        Setup::privkey_path(),
        amount,
        tx_fee,
    ));
    println!("dao deposit: {}", output);
    output
}

fn prepare(setup: &Setup, amount: f64, tx_fee: f64) -> String {
    let output = setup.cli(&format!(
        "dao prepare --privkey-path {} --capacity {} --tx-fee {}",
        Setup::privkey_path(),
        amount,
        tx_fee,
    ));
    println!("dao prepare: {}", output);
    output
}

fn withdraw(setup: &Setup, amount: f64, tx_fee: f64) -> String {
    let output = setup.cli(&format!(
        "dao withdraw --privkey-path {} --capacity {} --tx-fee {}",
        Setup::privkey_path(),
        amount,
        tx_fee,
    ));
    println!("dao withdraw: {}", output);
    output
}

fn query_deposit_live_cells(setup: &Setup) -> String {
    ensure_sync(setup);
    let output = setup.cli(&format!(
        "dao query-deposited-live-cells --address {}",
        Setup::address().display_with_prefix(NetworkType::TestNet),
    ));
    println!("dao query-deposited-live-cells: {}", output);
    output
}

fn query_prepare_live_cells(setup: &Setup) -> String {
    ensure_sync(setup);
    let output = setup.cli(&format!(
        "dao query-prepared-live-cells --address {}",
        Setup::address().display_with_prefix(NetworkType::TestNet),
    ));
    println!("dao query-prepared-live-cells: {}", output);
    output
}

// It's dirty. Do you have any better ideas?
fn ensure_sync(setup: &Setup) {
    setup.cli(&format!(
        "dao withdraw --privkey-path {} --capacity 61 --tx-fee 0",
        Setup::privkey_path(),
    ));
}
