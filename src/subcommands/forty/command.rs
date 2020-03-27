use crate::subcommands::{CliSubCommand, FortySubCommand};
use crate::utils::{
    arg,
    arg_parser::{
        AddressParser, ArgParser, CapacityParser, FixedHashParser, OutPointParser,
        PrivkeyPathParser, PrivkeyWrapper,
    },
    other::{get_address, get_network_type},
    printer::{OutputFormat, Printable},
};
use ckb_crypto::secp::{SECP256K1, Pubkey};
use ckb_sdk::{constants::SIGHASH_TYPE_HASH, Address, AddressPayload, NetworkType};
use ckb_types::{
    packed::{Byte32, Script},
    prelude::*,
    H160, H256,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use std::collections::HashSet;
use ckb_types::packed::{OutPoint, Bytes};
use crate::utils::arg_parser::PubkeyHexParser;
use secp256k1::PublicKey;
use sha2::Digest;
use crate::utils::other::serialize_signature;

impl<'a> CliSubCommand for FortySubCommand<'a> {
    fn process(
        &mut self,
        matches: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        let network_type = get_network_type(&mut self.rpc_client)?;
        match matches.subcommand() {
            ("issue", Some(m)) => {
                self.issue_args = Some(IssueArgs::from_matches(m, network_type)?);
                let transaction = self.issue()?;
                send_transaction(self.rpc_client(), transaction, format, color, debug)
            }
            ("transfer", Some(m)) => {
                self.transact_args = Some(TransactArgs::from_matches(m, network_type)?);
                let transaction = self.transfer()?;
                send_transaction(self.rpc_client(), transaction, format, color, debug)
            }
//            ("query", Some(m)) => {
//                let query_args = QueryArgs::from_matches(m, network_type)?;
//                let lock_hash = query_args.lock_hash;
//                let cells = self.query_prepare_cells(lock_hash)?;
//                let resp = serde_json::json!({
//                    "live_cells": (0..cells.len()).map(|i| {
//                        let mut value = serde_json::to_value(&cells[i]).unwrap();
//                        let obj = value.as_object_mut().unwrap();
//                        obj.insert("maximum_withdraw".to_owned(), serde_json::json!(maximum_withdraws[i]));
//                        value
//                    }).collect::<Vec<_>>(),
//                });
//                Ok(resp.render(format, color))
//            }
            _ => Err(matches.usage().to_owned()),
        }
    }
}

impl<'a> FortySubCommand<'a> {
    pub fn subcommand() -> App<'static, 'static> {
        SubCommand::with_name("dao")
            .about("FortyToken operations")
            .subcommand(
                SubCommand::with_name("issue")
                    .about("Issue FT to admin self")
                    .arg(arg::privkey_path().required_unless(arg::from_account().b.name))
                    .arg(arg::amount().required(true))
                    .arg(arg::nonce().required(true)),
            )
            .subcommand(
                SubCommand::with_name("transfer")
                    .about("Transfer FT")
                    .arg(arg::privkey_path().required(true))
                    .arg(arg::out_point().required(true))
                    .arg(arg::pubkey().required(true))
                    .arg(arg::amount().required(true))
                    .arg(arg::nonce().required(true)),
            )
    }
}

//pub(crate) struct QueryArgs {
//    pub(crate) lock_hash: Byte32,
//}

pub(crate) struct IssueArgs {
    pub(crate) network_type: NetworkType,
    pub(crate) sender: PrivkeyWrapper,
    pub(crate) amount: u64,
    pub(crate) nonce: u64,
}

pub(crate) struct TransactArgs {
    pub(crate) network_type: NetworkType,
    pub(crate) sender: PrivkeyWrapper,
    pub(crate) receiver: PublicKey,
    pub(crate) amount: u64,
    pub(crate) nonce: u64,
    pub(crate) out_point: OutPoint,
}

impl IssueArgs {
    fn from_matches(m: &ArgMatches, network_type: NetworkType) -> Result<Self, String> {
        let sender: PrivkeyWrapper =
            PrivkeyPathParser.from_matches_opt(m, "privkey-path", true)?.unwrap();
        let amount = m.value_of("amount").expect("expect amount").parse()
            .map_err(|err| err.to_string())?;
        let nonce = m.value_of("nonce").expect("expect nonce").parse()
            .map_err(|err| err.to_string())?;
        Ok(Self {
            network_type,
            sender,
            amount,
            nonce,
        })
    }

    pub fn sender_privkey(&self) -> &PrivkeyWrapper {
        &self.sender
    }

    pub fn receiver_address(&self) -> Address {
        let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &self.sender);
        let payload = AddressPayload::from_pubkey(&pubkey);
        Address::new(self.network_type, payload)
    }

    pub(crate) fn receiver_sighash_args(&self) -> H160 {
        H160::from_slice(self.receiver_address().payload().args().as_ref()).unwrap()
    }

    pub(crate) fn receiver_lock_hash(&self) -> Byte32 {
        Script::from(self.receiver_address().payload()).calc_script_hash()
    }

    pub(crate) fn amount_hash(&self) -> Bytes {
        let mut hasher = sha2::Sha256::new();
        let preimage = format!("{},{}", self.amount, self.nonce);
        hasher.input(preimage.as_bytes());
        let result = hasher.result();
        result.as_slice().pack()
    }

    // encrypted_amount = receiver.pubkey.sign_recoverable()
    pub(crate) fn encrypted_amount(&self) -> Bytes {
        // As for command "issue", the sender and receiver are the same.
        let receiver = &self.sender;

        let preimage = format!("{},{}", self.amount, self.nonce);
        let message = secp256k1::Message::from_slice(preimage.as_bytes())
            .expect("Failed to convert FT preimage to secp256k1 message");

        // FIXME 我不知道如何用 pubkey 加密 preimage，先留个FIXME，先折腾其它的
        let builder: secp256k1::Secp256k1<secp256k1::All> = secp256k1::Secp256k1::new();
        let signature = builder.sign_recoverable(&message, receiver);
        let serialized_signature = serialize_signature(&signature);
        serialized_signature.pack()
        // Bytes::from(serialized_signature[..].to_vec())
    }

    pub(crate) fn sender_address(&self) -> Address {
        self.receiver_address()
    }

    pub(crate) fn sender_sighash_args(&self) -> H160 {
        self.receiver_sighash_args()
    }

    pub(crate) fn sender_lock_hash(&self) -> Byte32 {
        self.receiver_lock_hash()
    }
}

impl TransactArgs {
    fn from_matches(m: &ArgMatches, network_type: NetworkType) -> Result<Self, String> {
        let sender: PrivkeyWrapper =
            PrivkeyPathParser.from_matches(m, "privkey-path")?.unwrap();
        let receiver = PubkeyHexParser.from_matches(m, "pubkey").unwrap();
        let amount = m.value_of("amount").expect("expect amount").parse()
            .map_err(|err| err.to_string())?;
        let nonce = m.value_of("nonce").expect("expect nonce").parse()
            .map_err(|err| err.to_string())?;
        let out_point: OutPoint = OutPointParser.from_matches(m, "out-point")?;
        Ok(Self {
            network_type,
            sender,
            receiver,
            amount,
            nonce,
            out_point,
        })
    }

    pub fn sender_privkey(&self) -> &PrivkeyWrapper {
        &self.sender
    }

    pub fn receiver_address(&self) -> Address {
        let payload = AddressPayload::from_pubkey(&self.receiver);
        Address::new(self.network_type, payload)
    }

    pub(crate) fn receiver_sighash_args(&self) -> H160 {
        H160::from_slice(self.receiver_address().payload().args().as_ref()).unwrap()
    }

    pub(crate) fn receiver_lock_hash(&self) -> Byte32 {
        Script::from(self.receiver_address().payload()).calc_script_hash()
    }

    pub(crate) fn receiver_encrypt_amount(&self) {
    }

    pub fn sender_address(&self) -> Address {
        let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &self.sender);
        let payload = AddressPayload::from_pubkey(&pubkey);
        Address::new(self.network_type, payload)
    }

    pub(crate) fn sender_sighash_args(&self) -> H160 {
        H160::from_slice(self.sender_address().payload().args().as_ref()).unwrap()
    }

    pub(crate) fn sender_lock_hash(&self) -> Byte32 {
        Script::from(self.sender_address().payload()).calc_script_hash()
    }
}

//impl QueryArgs {
//    fn from_matches(m: &ArgMatches, network_type: NetworkType) -> Result<Self, String> {
//        let lock_hash_opt: Option<H256> =
//            FixedHashParser::<H256>::default().from_matches_opt(m, "lock-hash", false)?;
//        let lock_hash = if let Some(lock_hash) = lock_hash_opt {
//            lock_hash.pack()
//        } else {
//            let address = get_address(Some(network_type), m)?;
//            Script::from(&address).calc_script_hash()
//        };
//
//        Ok(Self { lock_hash })
//    }
//
//    fn args<'a, 'b>() -> Vec<Arg<'a, 'b>> {
//        vec![arg::lock_hash(), arg::address()]
//    }
//}
//
