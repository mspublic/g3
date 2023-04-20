/*
 * Copyright 2023 ByteDance and/or its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::anyhow;
use clap::{value_parser, Arg, ArgAction, ArgGroup, ArgMatches, Command, ValueHint};
use openssl::bn::BigNumContext;
use openssl::ec::PointConversionForm;
use openssl::hash::{DigestBytes, MessageDigest};
use openssl::nid::Nid;
use openssl::pkey::{Id, PKey, Private};
use openssl::rsa::Padding;
use openssl::sign::Signer;
use openssl::x509::X509;

const ARG_CERT: &str = "cert";
const ARG_PKEY: &str = "key";
const ARG_RSA_PRIVATE_DECRYPT: &str = "rsa-private-decrypt";
const ARG_RSA_PRIVATE_ENCRYPT: &str = "rsa-private-encrypt";
const ARG_RSA_PUBLIC_DECRYPT: &str = "rsa-public-decrypt";
const ARG_RSA_PUBLIC_ENCRYPT: &str = "rsa-public-encrypt";
const ARG_SIGN: &str = "sign";
const ARG_DIGEST_TYPE: &str = "digest-type";
const ARG_RSA_PADDING: &str = "rsa-padding";
const ARG_PAYLOAD: &str = "payload";
const ARG_DUMP_RESULT: &str = "dump-result";

const DIGEST_TYPES: [&str; 6] = ["md5sha1", "sha1", "sha224", "sha256", "sha384", "sha512"];
const RSA_PADDING_VALUES: [&str; 5] = ["PKCS1", "OAEP", "PSS", "X931", "NONE"];

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum KeylessRsaPadding {
    #[default]
    Pkcs1,
    Oaep,
    Pss,
    X931,
    None,
}

impl FromStr for KeylessRsaPadding {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pkcs1" => Ok(KeylessRsaPadding::Pkcs1),
            "oaep" => Ok(KeylessRsaPadding::Oaep),
            "pss" => Ok(KeylessRsaPadding::Pss),
            "x931" => Ok(KeylessRsaPadding::X931),
            "none" => Ok(KeylessRsaPadding::None),
            _ => Err(anyhow!("unsupported rsa padding type {s}")),
        }
    }
}

impl From<KeylessRsaPadding> for Padding {
    fn from(value: KeylessRsaPadding) -> Self {
        match value {
            KeylessRsaPadding::None => Padding::NONE,
            KeylessRsaPadding::Pkcs1 => Padding::PKCS1,
            KeylessRsaPadding::Oaep => Padding::PKCS1_OAEP,
            KeylessRsaPadding::Pss => Padding::from_raw(6),
            KeylessRsaPadding::X931 => Padding::from_raw(5),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum KeylessSignDigest {
    Md5Sha1,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

impl KeylessSignDigest {
    fn check_payload(&self, payload: &[u8]) -> anyhow::Result<()> {
        let digest = MessageDigest::from(*self);
        if digest.size() != payload.len() {
            return Err(anyhow!(
                "payload size {} not match digest size {}",
                payload.len(),
                digest.size()
            ));
        }
        Ok(())
    }
}

impl FromStr for KeylessSignDigest {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "md5sha1" => Ok(KeylessSignDigest::Md5Sha1),
            "sha1" => Ok(KeylessSignDigest::Sha1),
            "sha224" => Ok(KeylessSignDigest::Sha224),
            "sha256" => Ok(KeylessSignDigest::Sha256),
            "sha384" => Ok(KeylessSignDigest::Sha384),
            "sha512" => Ok(KeylessSignDigest::Sha512),
            _ => Err(anyhow!("unsupported digest type {s}")),
        }
    }
}

impl From<KeylessSignDigest> for MessageDigest {
    fn from(value: KeylessSignDigest) -> Self {
        match value {
            KeylessSignDigest::Md5Sha1 => MessageDigest::from_nid(Nid::MD5_SHA1).unwrap(),
            KeylessSignDigest::Sha1 => MessageDigest::sha1(),
            KeylessSignDigest::Sha224 => MessageDigest::sha224(),
            KeylessSignDigest::Sha256 => MessageDigest::sha256(),
            KeylessSignDigest::Sha384 => MessageDigest::sha384(),
            KeylessSignDigest::Sha512 => MessageDigest::sha512(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum KeylessAction {
    RsaPrivateDecrypt(KeylessRsaPadding),
    RsaPrivateEncrypt(KeylessRsaPadding),
    RsaPublicDecrypt(KeylessRsaPadding),
    RsaPublicEncrypt(KeylessRsaPadding),
    RsaSign(KeylessSignDigest, KeylessRsaPadding),
    EcdsaSign(KeylessSignDigest),
    Ed25519Sign,
}

pub(super) trait AppendKeylessArgs {
    fn append_keyless_args(self) -> Self;
}

pub(super) struct KeylessGlobalArgs {
    pub(super) cert: X509,
    pub(super) key: Option<PKey<Private>>,
    pub(super) action: KeylessAction,
    pub(super) payload: Vec<u8>,
    dump_result: bool,
}

impl KeylessGlobalArgs {
    pub(super) fn parse_args(args: &ArgMatches) -> anyhow::Result<Self> {
        let Some(file) = args.get_one::<PathBuf>(ARG_CERT) else {
            unreachable!();
        };
        let cert = crate::target::tls::load_certs(file)?.pop().unwrap();
        let pkey = cert
            .public_key()
            .map_err(|e| anyhow!("failed to fetch pubkey: {e}"))?;

        let payload_str = args.get_one::<String>(ARG_PAYLOAD).unwrap();
        let payload = hex::decode(payload_str)
            .map_err(|e| anyhow!("the payload string is not valid hex string: {e}"))?;

        let rsa_padding = if let Some(s) = args.get_one::<String>(ARG_RSA_PADDING) {
            KeylessRsaPadding::from_str(s)?
        } else {
            KeylessRsaPadding::default()
        };

        let action = if args.get_flag(ARG_RSA_PRIVATE_DECRYPT) {
            KeylessAction::RsaPrivateDecrypt(rsa_padding)
        } else if args.get_flag(ARG_RSA_PRIVATE_ENCRYPT) {
            KeylessAction::RsaPrivateEncrypt(rsa_padding)
        } else if args.get_flag(ARG_RSA_PUBLIC_DECRYPT) {
            KeylessAction::RsaPublicDecrypt(rsa_padding)
        } else if args.get_flag(ARG_RSA_PUBLIC_ENCRYPT) {
            KeylessAction::RsaPublicEncrypt(rsa_padding)
        } else if args.get_flag(ARG_SIGN) {
            let digest_str = args.get_one::<String>(ARG_DIGEST_TYPE).unwrap();
            let digest_type = KeylessSignDigest::from_str(digest_str)?;

            match pkey.id() {
                Id::RSA => {
                    digest_type.check_payload(payload.as_slice())?;
                    KeylessAction::RsaSign(digest_type, rsa_padding)
                }
                Id::EC => {
                    digest_type.check_payload(payload.as_slice())?;
                    KeylessAction::EcdsaSign(digest_type)
                }
                Id::ED25519 => KeylessAction::Ed25519Sign,
                id => return Err(anyhow!("unsupported public key type {id:?}")),
            }
        } else {
            return Err(anyhow!("no keyless action set"));
        };

        let dump_result = args.get_flag(ARG_DUMP_RESULT);

        let mut key_args = KeylessGlobalArgs {
            cert,
            key: None,
            action,
            payload,
            dump_result,
        };

        if let Some(file) = args.get_one::<PathBuf>(ARG_PKEY) {
            let key = crate::target::tls::load_key(file)?;
            key_args.key = Some(key);
        }

        Ok(key_args)
    }

    pub(super) fn dump_result(&self, task_id: usize, data: Vec<u8>) {
        if self.dump_result {
            let hex_str = hex::encode(data);
            println!("== Output of task {task_id}:\n{hex_str}");
        }
    }

    pub(super) fn get_public_key_digest(&self) -> anyhow::Result<DigestBytes> {
        let pkey = self
            .cert
            .public_key()
            .map_err(|e| anyhow!("no public key found in cert: {e}"))?;
        if let Ok(rsa) = pkey.rsa() {
            let hex = rsa
                .n()
                .to_hex_str()
                .map_err(|e| anyhow!("failed to get hex string of rsa modulus: {e}"))?;
            openssl::hash::hash(MessageDigest::sha256(), hex.as_bytes())
                .map_err(|e| anyhow!("public key digest hash error: {e}"))
        } else if let Ok(ec) = pkey.ec_key() {
            let group = ec.group();
            let point = ec.public_key();
            let mut ctx = BigNumContext::new_secure().unwrap();
            let bytes = point
                .to_bytes(group, PointConversionForm::COMPRESSED, &mut ctx)
                .unwrap();
            let hex = hex::encode(bytes);
            openssl::hash::hash(MessageDigest::sha256(), hex.as_bytes())
                .map_err(|e| anyhow!("public key digest hash error: {e}"))
        } else {
            Err(anyhow!("unsupported public type: {:?}", pkey.id()))
        }
    }

    pub(super) fn rsa_private_decrypt(
        &self,
        padding: KeylessRsaPadding,
    ) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("no private key set"))?;
        let rsa = pkey
            .rsa()
            .map_err(|e| anyhow!("private key is not rsa: {e}"))?;

        let rsa_size = rsa.size() as usize;
        let mut output_buf = Vec::new();
        output_buf.resize(rsa_size, 0);

        let payload_len = self.payload.len();
        if payload_len != rsa_size {
            return Err(anyhow!(
                "payload length {payload_len} is not equal to RSA size {rsa_size}"
            ));
        }

        let len = rsa
            .private_decrypt(&self.payload, &mut output_buf, padding.into())
            .map_err(|e| anyhow!("rsa private decrypt failed: {e}"))?;
        output_buf.resize(len, 0);
        Ok(output_buf)
    }

    pub(super) fn rsa_private_encrypt(
        &self,
        padding: KeylessRsaPadding,
    ) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("no private key set"))?;
        let rsa = pkey
            .rsa()
            .map_err(|e| anyhow!("private key is not rsa: {e}"))?;

        let rsa_size = rsa.size() as usize;
        let mut output_buf = Vec::new();
        output_buf.resize(rsa_size, 0);

        let payload_len = self.payload.len();
        if payload_len > rsa_size {
            return Err(anyhow!(
                "payload length {payload_len} is larger than RSA size {rsa_size}"
            ));
        }

        let len = rsa
            .private_decrypt(&self.payload, &mut output_buf, padding.into())
            .map_err(|e| anyhow!("rsa private encrypt failed: {e}"))?;
        output_buf.resize(len, 0);
        Ok(output_buf)
    }

    pub(super) fn rsa_public_decrypt(&self, padding: KeylessRsaPadding) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .cert
            .public_key()
            .map_err(|e| anyhow!("no valid pkey found in cert: {e}"))?;
        let rsa = pkey
            .rsa()
            .map_err(|e| anyhow!("the cert is not a valid rsa cert: {e}"))?;

        let rsa_size = rsa.size() as usize;
        let mut output_buf = Vec::new();
        output_buf.resize(rsa_size, 0);

        let payload_len = self.payload.len();
        if payload_len != rsa_size {
            return Err(anyhow!(
                "payload length {payload_len} is not equal to RSA size {rsa_size}"
            ));
        }

        let len = rsa
            .public_decrypt(&self.payload, &mut output_buf, padding.into())
            .map_err(|e| anyhow!("rsa public decrypt failed: {e}"))?;
        output_buf.resize(len, 0);
        Ok(output_buf)
    }

    pub(super) fn rsa_public_encrypt(&self, padding: KeylessRsaPadding) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .cert
            .public_key()
            .map_err(|e| anyhow!("no valid pkey found in cert: {e}"))?;
        let rsa = pkey
            .rsa()
            .map_err(|e| anyhow!("the cert is not a valid rsa cert: {e}"))?;

        let rsa_size = rsa.size() as usize;
        let mut output_buf = Vec::new();
        output_buf.resize(rsa_size, 0);

        let payload_len = self.payload.len();
        if payload_len > rsa_size {
            return Err(anyhow!(
                "payload length {payload_len} is larger than RSA size {rsa_size}"
            ));
        }

        let len = rsa
            .public_encrypt(&self.payload, &mut output_buf, padding.into())
            .map_err(|e| anyhow!("rsa public encrypt failed: {e}"))?;
        output_buf.resize(len, 0);
        Ok(output_buf)
    }

    pub(super) fn pkey_sign(&self, digest: KeylessSignDigest) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("no private key set"))?;

        let mut signer = Signer::new(digest.into(), pkey)
            .map_err(|e| anyhow!("error when create signer: {e}"))?;
        signer
            .update(&self.payload)
            .map_err(|e| anyhow!("failed to set payload data: {e}"))?;
        signer
            .sign_to_vec()
            .map_err(|e| anyhow!("sign failed: {e}"))
    }

    pub(super) fn pkey_sign_rsa(
        &self,
        digest: KeylessSignDigest,
        padding: KeylessRsaPadding,
    ) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("no private key set"))?;

        let mut signer = Signer::new(digest.into(), pkey)
            .map_err(|e| anyhow!("error when create signer: {e}"))?;
        signer
            .set_rsa_padding(padding.into())
            .map_err(|e| anyhow!("failed to set rsa padding: {e}"))?;
        signer
            .update(&self.payload)
            .map_err(|e| anyhow!("failed to set payload data: {e}"))?;
        signer
            .sign_to_vec()
            .map_err(|e| anyhow!("sign failed: {e}"))
    }

    pub(super) fn pkey_sign_ed(&self) -> anyhow::Result<Vec<u8>> {
        let pkey = self
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("no private key set"))?;

        let mut signer = Signer::new_without_digest(pkey)
            .map_err(|e| anyhow!("error when create signer: {e}"))?;
        signer
            .update(&self.payload)
            .map_err(|e| anyhow!("failed to set payload data: {e}"))?;
        signer
            .sign_to_vec()
            .map_err(|e| anyhow!("sign failed: {e}"))
    }
}

fn add_keyless_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new(ARG_CERT)
            .help("Target certificate file")
            .num_args(1)
            .long(ARG_CERT)
            .value_parser(value_parser!(PathBuf))
            .required(true)
            .value_hint(ValueHint::FilePath),
    )
    .arg(
        Arg::new(ARG_PKEY)
            .help("Target private key file")
            .num_args(1)
            .long(ARG_PKEY)
            .value_parser(value_parser!(PathBuf))
            .value_hint(ValueHint::FilePath),
    )
    .arg(
        Arg::new(ARG_RSA_PRIVATE_DECRYPT)
            .help("RSA Private Decrypt")
            .num_args(0)
            .long(ARG_RSA_PRIVATE_DECRYPT)
            .action(ArgAction::SetTrue)
            .requires(ARG_RSA_PADDING),
    )
    .arg(
        Arg::new(ARG_RSA_PRIVATE_ENCRYPT)
            .help("RSA Private Encrypt")
            .num_args(0)
            .long(ARG_RSA_PRIVATE_ENCRYPT)
            .action(ArgAction::SetTrue)
            .requires(ARG_RSA_PADDING),
    )
    .arg(
        Arg::new(ARG_RSA_PUBLIC_DECRYPT)
            .help("RSA Public Decrypt")
            .num_args(0)
            .long(ARG_RSA_PUBLIC_DECRYPT)
            .action(ArgAction::SetTrue)
            .requires(ARG_RSA_PADDING),
    )
    .arg(
        Arg::new(ARG_RSA_PUBLIC_ENCRYPT)
            .help("RSA Public Encrypt")
            .num_args(0)
            .long(ARG_RSA_PUBLIC_ENCRYPT)
            .action(ArgAction::SetTrue)
            .requires(ARG_RSA_PADDING),
    )
    .arg(
        Arg::new(ARG_SIGN)
            .help("Computes cryptographic signatures of data")
            .num_args(0)
            .long(ARG_SIGN)
            .action(ArgAction::SetTrue)
            .requires(ARG_DIGEST_TYPE),
    )
    .group(
        ArgGroup::new("method")
            .args([
                ARG_RSA_PRIVATE_DECRYPT,
                ARG_RSA_PRIVATE_ENCRYPT,
                ARG_RSA_PUBLIC_DECRYPT,
                ARG_RSA_PUBLIC_ENCRYPT,
                ARG_SIGN,
            ])
            .required(true),
    )
    .arg(
        Arg::new(ARG_DIGEST_TYPE)
            .help("Sign Digest Type")
            .num_args(1)
            .long(ARG_DIGEST_TYPE)
            .value_parser(DIGEST_TYPES),
    )
    .arg(
        Arg::new(ARG_RSA_PADDING)
            .help("RSA Padding Type")
            .num_args(1)
            .long(ARG_RSA_PADDING)
            .value_parser(RSA_PADDING_VALUES)
            .default_value("PKCS1"),
    )
    .arg(
        Arg::new(ARG_PAYLOAD)
            .help("Payload data")
            .num_args(1)
            .required(true),
    )
    .arg(
        Arg::new(ARG_DUMP_RESULT)
            .help("Dump output use hex string")
            .action(ArgAction::SetTrue)
            .num_args(0)
            .long(ARG_DUMP_RESULT),
    )
}

impl AppendKeylessArgs for Command {
    fn append_keyless_args(self) -> Self {
        add_keyless_args(self)
    }
}