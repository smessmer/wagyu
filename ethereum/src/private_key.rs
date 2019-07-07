use crate::address::EthereumAddress;
use model::{
    //    bytes::{FromBytes, ToBytes},
    //    crypto::checksum,
    Address,
    PrivateKey,
    PublicKey,
};
use crate::public_key::EthereumPublicKey;

use rand::rngs::OsRng;
use rand::Rng;
use secp256k1;
use secp256k1::Secp256k1;
//use std::io::{Read, Result as IoResult, Write};
use std::{fmt, fmt::Display};
use std::marker::PhantomData;
use std::str::FromStr;

/// Represents an Ethereum private key
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EthereumPrivateKey {
    /// The ECDSA private key
    pub secret_key: secp256k1::SecretKey,

    /// The Wallet Import Format (WIF) string encoding
    pub wif: String,
}

impl PrivateKey for EthereumPrivateKey {
    type Address = EthereumAddress;
    type Format = PhantomData<u8>;
    type Network = PhantomData<u8>;
    type PublicKey = EthereumPublicKey;

    /// Returns a randomly-generated Ethereum private key.
    fn new(_network: &Self::Network) -> Self {
        Self::build()
    }

    /// Returns the public key of the corresponding Ethereum private key.
    fn to_public_key(&self) -> Self::PublicKey {
        EthereumPublicKey::from_private_key(self)
    }

    /// Returns the address of the corresponding Ethereum private key.
    fn to_address(&self, _: &Self::Format) -> Self::Address {
        EthereumAddress::from_private_key(self, &PhantomData)
    }
}

impl EthereumPrivateKey {
    /// Returns a private key given a secp256k1 secret key
    pub fn from_secret_key(secret_key: secp256k1::SecretKey) -> Self {
        let wif = Self::secret_key_to_wif(&secret_key);
        Self { secret_key, wif }
    }

    /// Returns either a Ethereum private key struct or errors.
    pub fn from_wif(wif: &str) -> Result<Self, &'static str> {
        let secret_key = hex::decode(wif).expect("Error decoding wif (invalid hex string)");
        let secret_key = secp256k1::SecretKey::from_slice(&Secp256k1::new(), &secret_key)
            .expect("Error converting byte slice to secret key");
        Ok(Self { wif: wif.into(), secret_key })
    }

    /// Returns a randomly-generated Ethereum private key.
    fn build() -> Self {
        let secret_key = Self::random_secret_key();
        let wif = Self::secret_key_to_wif(&secret_key);
        Self { secret_key, wif }
    }

    /// Returns a randomly-generated secp256k1 secret key.
    fn random_secret_key() -> secp256k1::SecretKey {
        let mut random = [0u8; 32];
        OsRng.try_fill(&mut random).expect("Error generating random bytes for private key");
        secp256k1::SecretKey::from_slice(&Secp256k1::new(), &random)
            .expect("Error creating secret key from byte slice")
    }

    /// Returns a hex string representing a secp256k1 secret key.
    fn secret_key_to_wif(secret_key: &secp256k1::SecretKey) -> String {
        let mut secret_key_bytes = [0u8; 32];
        secret_key_bytes.copy_from_slice(&secret_key[..]);
        hex::encode(secret_key_bytes).to_string()
    }
}

impl Default for EthereumPrivateKey {
    /// Returns a randomly-generated Ethereum private key.
    fn default() -> Self {
        Self::new(&PhantomData)
    }
}

//impl FromBytes for EthereumPrivateKey {
//    #[inline]
//    fn read<R: Read>(reader: R) -> IoResult<Self> {
//        let mut f = reader;
//        let mut buffer = Vec::new();
//        f.read_to_end(&mut buffer)?;
//
//        Self::from_str(buffer.to_base58().as_str())?
//    }
//}
//
//impl ToBytes for EthereumPrivateKey {
//    #[inline]
//    fn write<W: Write>(&self, writer: W) -> IoResult<()> {
//        let buffer = self.wif.as_str().from_base58()?.as_slice();
//        buffer.write(writer)
//    }
//}

impl FromStr for EthereumPrivateKey {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, &'static str> {
        Self::from_wif(s)
    }
}

impl Display for EthereumPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.wif)
    }
}