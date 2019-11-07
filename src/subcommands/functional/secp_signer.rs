use crate::subcommands::functional::IndexClient;
use ckb_crypto::secp::SECP256K1;
use ckb_hash::new_blake2b;
use ckb_sdk::wallet::KeyStoreError;
use ckb_types::bytes::Bytes;
use ckb_types::packed::{Byte32, WitnessArgs};
use ckb_types::prelude::*;
use ckb_types::{H160, H256};
use secp256k1::recovery::RecoverableSignature;

pub fn build_secp_witnesses<F>(
    tx_hash: Byte32,
    mut witnesses: Vec<Bytes>,
    mut signer: F,
) -> Result<Vec<Bytes>, String>
where
    F: FnMut(&[u8; 32]) -> Result<Bytes, String>,
{
    let init_witness = if witnesses[0].is_empty() {
        WitnessArgs::default()
    } else {
        WitnessArgs::from_slice(&witnesses[0]).map_err(|err| err.to_string())?
    };
    assert!(init_witness.lock().is_none());
    let init_witness = init_witness
        .as_builder()
        .lock(Some(Bytes::from(&[0u8; 65][..])).pack())
        .build();
    let mut sign_args = vec![
        tx_hash.raw_data().to_vec(),
        (init_witness.as_bytes().len() as u64)
            .to_le_bytes()
            .to_vec(),
        init_witness.as_bytes().to_vec(),
    ];
    for other_witness in witnesses.iter().skip(1) {
        sign_args.push((other_witness.len() as u64).to_le_bytes().to_vec());
        sign_args.push(other_witness.to_vec());
    }
    let digest = blake2b_args(&sign_args);
    let signature = signer(&digest)?;
    let final_witness = init_witness
        .as_builder()
        .lock(Some(signature).pack())
        .build();
    witnesses[0] = final_witness.as_bytes();
    Ok(witnesses)
}

pub fn sign_with_privkey(
    privkey: &secp256k1::SecretKey,
    digest: &[u8; 32],
) -> Result<Bytes, String> {
    let message =
        secp256k1::Message::from_slice(digest).expect("Convert to secp256k1 message failed");
    Ok(serialize_signature(
        &SECP256K1.sign_recoverable(&message, privkey),
    ))
}

pub fn sign_with_keystore(
    index_client: &mut IndexClient,
    lock_arg: &H160,
    password: &Option<String>,
    interactive: bool,
    digest: &[u8; 32],
) -> Result<Bytes, String> {
    let sign_hash =
        H256::from_slice(digest).expect("converting digest of [u8; 32] to H256 should be ok");

    if let Some(password) = password {
        return index_client
            .key_store()
            .sign_recoverable_with_password(lock_arg, &sign_hash, password.as_bytes())
            .map_err(|err| err.to_string())
            .map(|signature| serialize_signature(&signature));
    }

    if interactive {
        return index_client.key_store()
            .sign_recoverable(lock_arg, &sign_hash)
            .map_err(|err| {
                match err {
                    KeyStoreError::AccountLocked(lock_arg) => {
                        format!("Account(lock_arg={:x}) locked or not exists, your may use `account unlock` to unlock it or use --with-password", lock_arg)
                    }
                    err => err.to_string(),
                }
            }).map(|signature| serialize_signature(&signature));
    }

    Err("Password required to unlock the keystore".to_owned())
}

pub fn serialize_signature(signature: &RecoverableSignature) -> Bytes {
    let (recov_id, data) = signature.serialize_compact();
    let mut signature_bytes = [0u8; 65];
    signature_bytes[0..64].copy_from_slice(&data[0..64]);
    signature_bytes[64] = recov_id.to_i32() as u8;
    Bytes::from(signature_bytes.to_vec())
}

pub fn blake2b_args(args: &[Vec<u8>]) -> [u8; 32] {
    let mut blake2b = new_blake2b();
    for arg in args.iter() {
        blake2b.update(&arg);
    }
    let mut digest = [0u8; 32];
    blake2b.finalize(&mut digest);
    digest
}
