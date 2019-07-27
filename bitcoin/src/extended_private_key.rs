use crate::address::{BitcoinAddress, Format};
use crate::derivation_path::BitcoinDerivationPath;
use crate::extended_public_key::BitcoinExtendedPublicKey;
use crate::network::BitcoinNetwork;
use crate::private_key::BitcoinPrivateKey;
use crate::public_key::BitcoinPublicKey;
use wagu_model::{
    AddressError,
    ChildIndex,
    ExtendedPublicKey,
    ExtendedPrivateKey,
    ExtendedPrivateKeyError,
    PrivateKey,
    crypto::{checksum, hash160}
};

use base58::{FromBase58, ToBase58};
use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use hmac::{Hmac, Mac};
use secp256k1::{Secp256k1, SecretKey, PublicKey};
use sha2::Sha512;
use std::{fmt, fmt::Display};
use std::io::Cursor;
use std::marker::PhantomData;
use std::str::FromStr;

type HmacSha512 = Hmac<Sha512>;

/// Represents a Bitcoin extended private key
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BitcoinExtendedPrivateKey<N: BitcoinNetwork> {
    /// The address format
    pub format: Format,
    /// The depth of key derivation, e.g. 0x00 for master nodes, 0x01 for level-1 derived keys, ...
    pub depth: u8,
    /// The first 32 bits of the key identifier (hash160(ECDSA_public_key))
    pub parent_fingerprint: [u8; 4],
    /// The child index of the key (0 for master key)
    pub child_index: ChildIndex,
    /// The chain code for this extended private key
    pub chain_code: [u8; 32],
    /// The Bitcoin private key
    pub private_key: BitcoinPrivateKey<N>,
    /// PhantomData
    _network: PhantomData<N>
}

impl <N: BitcoinNetwork> ExtendedPrivateKey for BitcoinExtendedPrivateKey<N> {
    type Address = BitcoinAddress<N>;
    type DerivationPath = BitcoinDerivationPath;
    type ExtendedPublicKey = BitcoinExtendedPublicKey<N>;
    type Format = Format;
    type PrivateKey = BitcoinPrivateKey<N>;
    type PublicKey = BitcoinPublicKey<N>;

    /// Returns a new Bitcoin extended private key.
    fn new(
        seed: &[u8],
        format: &Self::Format,
        path: &Self::DerivationPath,
    ) -> Result<Self, ExtendedPrivateKeyError> {
        Ok(Self::new_master(seed, format)?.derive(path)?)
    }

    /// Returns a new Bitcoin extended private key.
    fn new_master(seed: &[u8], format: &Self::Format) -> Result<Self, ExtendedPrivateKeyError> {
        let mut mac = HmacSha512::new_varkey(b"Bitcoin seed")?;
        mac.input(seed);
        let hmac = mac.result().code();
        let private_key =
            Self::PrivateKey::from_secret_key(SecretKey::from_slice(&hmac[0..32])?, true);

        let mut chain_code = [0u8; 32];
        chain_code[0..32].copy_from_slice(&hmac[32..]);

        Ok(Self {
            format: format.clone(),
            depth: 0,
            parent_fingerprint: [0u8; 4],
            child_index: ChildIndex::Normal(0),
            chain_code,
            private_key,
            _network: PhantomData
        })
    }

    /// Returns the extended private key of the given derivation path.
    fn derive(&self, path: &Self::DerivationPath) -> Result<Self, ExtendedPrivateKeyError> {
        if self.depth == 255 {
            return Err(ExtendedPrivateKeyError::MaximumChildDepthReached(self.depth))
        }

        let mut extended_private_key = self.clone();

        for index in path.0.iter() {
            let public_key = &PublicKey::from_secret_key(
                &Secp256k1::new(), &extended_private_key.private_key.secret_key).serialize()[..];

            let mut mac = HmacSha512::new_varkey(&extended_private_key.chain_code)?;
            match index {
                // HMAC-SHA512(Key = cpar, Data = serP(point(kpar)) || ser32(i)).
                ChildIndex::Normal(_) => mac.input(public_key),
                // HMAC-SHA512(Key = cpar, Data = 0x00 || ser256(kpar) || ser32(i))
                // (Note: The 0x00 pads the private key to make it 33 bytes long.)
                ChildIndex::Hardened(_) => {
                    mac.input(&[0u8]);
                    mac.input(&extended_private_key.private_key.secret_key[..]);
                }
            }
            // Append the child index in big-endian format
            let mut index_be = [0u8; 4];
            BigEndian::write_u32(&mut index_be, u32::from(*index));
            mac.input(&index_be);
            let hmac = mac.result().code();

            let mut private_key =
                Self::PrivateKey::from_secret_key(SecretKey::from_slice(&hmac[0..32])?, true);
            private_key.secret_key.add_assign(&extended_private_key.private_key.secret_key[..])?;

            let mut chain_code = [0u8; 32];
            chain_code[0..32].copy_from_slice(&hmac[32..]);

            let mut parent_fingerprint = [0u8; 4];
            parent_fingerprint.copy_from_slice(&hash160(public_key)[0..4]);

            extended_private_key = Self {
                format: extended_private_key.format.clone(),
                depth: extended_private_key.depth + 1,
                parent_fingerprint,
                child_index: *index,
                chain_code,
                private_key,
                _network: PhantomData
            }
        }

        Ok(extended_private_key)
    }

    /// Returns the extended public key of the corresponding extended private key.
    fn to_extended_public_key(&self) -> Self::ExtendedPublicKey {
        Self::ExtendedPublicKey::from_extended_private_key(&self)
    }

    /// Returns the private key of the corresponding extended private key.
    fn to_private_key(&self) -> Self::PrivateKey {
        self.private_key.clone()
    }

    /// Returns the public key of the corresponding extended private key.
    fn to_public_key(&self) -> Self::PublicKey {
        self.private_key.to_public_key()
    }

    /// Returns the address of the corresponding extended private key.
    fn to_address(&self, format: &Self::Format) -> Result<Self::Address, AddressError> {
        self.private_key.to_address(format)
    }
}

impl <N: BitcoinNetwork> FromStr for BitcoinExtendedPrivateKey<N> {
    type Err = ExtendedPrivateKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let data = s.from_base58()?;
        if data.len() != 82 {
            return Err(ExtendedPrivateKeyError::InvalidByteLength(data.len()))
        }

        // Check that the version bytes correspond with the correct network.
        let _ = N::from_extended_private_key_version_bytes(&data[0..4])?;
        let format = Format::from_extended_private_key_version_bytes(&data[0..4])?;

        let depth = data[4];

        let mut parent_fingerprint = [0u8; 4];
        parent_fingerprint.copy_from_slice(&data[5..9]);

        let child_index = ChildIndex::from(Cursor::new(&data[9..13]).read_u32::<BigEndian>()?);

        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&data[13..45]);

        let private_key =
            BitcoinPrivateKey::from_secret_key(SecretKey::from_slice(&data[46..78])?, true);

        let expected = &data[78..82];
        let checksum = &checksum(&data[0..78])[0..4];
        if *expected != *checksum {
            let expected = expected.to_base58();
            let found = checksum.to_base58();
            return Err(ExtendedPrivateKeyError::InvalidChecksum(expected, found))
        }

        Ok(Self {
            format,
            depth,
            parent_fingerprint,
            child_index,
            chain_code,
            private_key,
            _network: PhantomData
        })
    }
}

impl <N: BitcoinNetwork> Display for BitcoinExtendedPrivateKey<N> {
    /// BIP32 serialization format
    /// https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki#serialization-format
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut result = [0u8; 82];
        result[0..4].copy_from_slice(match &N::to_extended_private_key_version_bytes(&self.format) {
            Ok(version) => version,
            Err(_) => return Err(fmt::Error)
        });
        result[4] = self.depth;
        result[5..9].copy_from_slice(&self.parent_fingerprint[..]);

        BigEndian::write_u32(&mut result[9..13], u32::from(self.child_index));

        result[13..45].copy_from_slice(&self.chain_code[..]);
        result[45] = 0;
        result[46..78].copy_from_slice(&self.private_key.secret_key[..]);

        let checksum = &checksum(&result[0..78])[0..4];
        result[78..82].copy_from_slice(&checksum);

        fmt.write_str(&result.to_base58())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::*;

    use hex;
    use std::string::String;

    fn test_new<N: BitcoinNetwork>(
        expected_extended_private_key: &str,
        expected_parent_fingerprint: &str,
        expected_child_index: u32,
        expected_chain_code: &str,
        expected_secret_key: &str,
        seed: &str,
        format: &Format,
        path: &BitcoinDerivationPath
    ) {
        let extended_private_key =
            BitcoinExtendedPrivateKey::<N>::new(&hex::decode(seed).unwrap(), format, path).unwrap();
        assert_eq!(expected_extended_private_key, extended_private_key.to_string());
        assert_eq!(expected_parent_fingerprint, hex::encode(extended_private_key.parent_fingerprint));
        assert_eq!(expected_child_index, u32::from(extended_private_key.child_index));
        assert_eq!(expected_chain_code, hex::encode(extended_private_key.chain_code));
        assert_eq!(expected_secret_key, extended_private_key.private_key.secret_key.to_string());
    }

    // Check: (extended_private_key1 -> extended_private_key2) == (expected_extended_private_key2)
    fn test_derive<N: BitcoinNetwork>(
        expected_extended_private_key1: &str,
        expected_extended_private_key2: &str,
        expected_child_index2: u32
    ) {
        let path = BitcoinDerivationPath(vec![ChildIndex::from(expected_child_index2)]);

        let extended_private_key1 = BitcoinExtendedPrivateKey::<N>::from_str(expected_extended_private_key1).unwrap();
        let extended_private_key2 = extended_private_key1.derive(&path).unwrap();

        let expected_extended_private_key2 = BitcoinExtendedPrivateKey::<N>::from_str(&expected_extended_private_key2).unwrap();

        assert_eq!(expected_extended_private_key2, extended_private_key2);
        assert_eq!(expected_extended_private_key2.private_key, extended_private_key2.private_key);
        assert_eq!(expected_extended_private_key2.child_index, extended_private_key2.child_index);
        assert_eq!(expected_extended_private_key2.chain_code, extended_private_key2.chain_code);
        assert_eq!(expected_extended_private_key2.parent_fingerprint, extended_private_key2.parent_fingerprint);
    }

    fn test_to_extended_public_key<N: BitcoinNetwork>(
        expected_extended_public_key: &str,
        seed: &str,
        format: &Format,
        path: &BitcoinDerivationPath
    ) {
        let extended_private_key =
            BitcoinExtendedPrivateKey::<N>::new(&hex::decode(seed).unwrap(), format, path).unwrap();
        let extended_public_key = extended_private_key.to_extended_public_key();
        assert_eq!(expected_extended_public_key, extended_public_key.to_string());
    }

    fn test_from_str<N: BitcoinNetwork>(
        expected_extended_private_key: &str,
        expected_parent_fingerprint: &str,
        expected_child_index: u32,
        expected_chain_code: &str,
        expected_secret_key: &str,
    ) {
        let extended_private_key =
            BitcoinExtendedPrivateKey::<N>::from_str(expected_extended_private_key).unwrap();
        assert_eq!(expected_extended_private_key, extended_private_key.to_string());
        assert_eq!(expected_parent_fingerprint, hex::encode(extended_private_key.parent_fingerprint));
        assert_eq!(expected_child_index, u32::from(extended_private_key.child_index));
        assert_eq!(expected_chain_code, hex::encode(extended_private_key.chain_code));
        assert_eq!(expected_secret_key, extended_private_key.private_key.secret_key.to_string());
    }

    fn test_to_string<N: BitcoinNetwork>(expected_extended_private_key: &str) {
        let extended_private_key =
            BitcoinExtendedPrivateKey::<N>::from_str(expected_extended_private_key).unwrap();
        assert_eq!(expected_extended_private_key, extended_private_key.to_string());
    }

    mod bip32_mainnet {
        use super::*;

        type N = Mainnet;

        // (path, seed, child_index, secret_key, chain_code, parent_fingerprint, extended_private_key, extended_public_key)
        const KEYPAIRS: [(&str, &str, &str, &str, &str, &str, &str, &str); 12] = [
            (
                "m",
                "000102030405060708090a0b0c0d0e0f",
                "0",
                "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35",
                "873dff81c02f525623fd1fe5167eac3a55a049de3d314bb42ee227ffed37d508",
                "00000000",
                "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi",
                "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8"
            ),
            (
                "m/0'",
                "000102030405060708090a0b0c0d0e0f",
                "2147483648",
                "edb2e14f9ee77d26dd93b4ecede8d16ed408ce149b6cd80b0715a2d911a0afea",
                "47fdacbd0f1097043b78c63c20c34ef4ed9a111d980047ad16282c7ae6236141",
                "3442193e",
                "xprv9uHRZZhk6KAJC1avXpDAp4MDc3sQKNxDiPvvkX8Br5ngLNv1TxvUxt4cV1rGL5hj6KCesnDYUhd7oWgT11eZG7XnxHrnYeSvkzY7d2bhkJ7",
                "xpub68Gmy5EdvgibQVfPdqkBBCHxA5htiqg55crXYuXoQRKfDBFA1WEjWgP6LHhwBZeNK1VTsfTFUHCdrfp1bgwQ9xv5ski8PX9rL2dZXvgGDnw"
            ),
            (
                "m/0'/1",
                "000102030405060708090a0b0c0d0e0f",
                "1",
                "3c6cb8d0f6a264c91ea8b5030fadaa8e538b020f0a387421a12de9319dc93368",
                "2a7857631386ba23dacac34180dd1983734e444fdbf774041578e9b6adb37c19",
                "5c1bd648",
                "xprv9wTYmMFdV23N2TdNG573QoEsfRrWKQgWeibmLntzniatZvR9BmLnvSxqu53Kw1UmYPxLgboyZQaXwTCg8MSY3H2EU4pWcQDnRnrVA1xe8fs",
                "xpub6ASuArnXKPbfEwhqN6e3mwBcDTgzisQN1wXN9BJcM47sSikHjJf3UFHKkNAWbWMiGj7Wf5uMash7SyYq527Hqck2AxYysAA7xmALppuCkwQ"
            ),
            (
                "m/0'/1/2'",
                "000102030405060708090a0b0c0d0e0f",
                "2147483650",
                "cbce0d719ecf7431d88e6a89fa1483e02e35092af60c042b1df2ff59fa424dca",
                "04466b9cc8e161e966409ca52986c584f07e9dc81f735db683c3ff6ec7b1503f",
                "bef5a2f9",
                "xprv9z4pot5VBttmtdRTWfWQmoH1taj2axGVzFqSb8C9xaxKymcFzXBDptWmT7FwuEzG3ryjH4ktypQSAewRiNMjANTtpgP4mLTj34bhnZX7UiM",
                "xpub6D4BDPcP2GT577Vvch3R8wDkScZWzQzMMUm3PWbmWvVJrZwQY4VUNgqFJPMM3No2dFDFGTsxxpG5uJh7n7epu4trkrX7x7DogT5Uv6fcLW5"
            ),
            (
                "m/0'/1/2'/2",
                "000102030405060708090a0b0c0d0e0f",
                "2",
                "0f479245fb19a38a1954c5c7c0ebab2f9bdfd96a17563ef28a6a4b1a2a764ef4",
                "cfb71883f01676f587d023cc53a35bc7f88f724b1f8c2892ac1275ac822a3edd",
                "ee7ab90c",
                "xprvA2JDeKCSNNZky6uBCviVfJSKyQ1mDYahRjijr5idH2WwLsEd4Hsb2Tyh8RfQMuPh7f7RtyzTtdrbdqqsunu5Mm3wDvUAKRHSC34sJ7in334",
                "xpub6FHa3pjLCk84BayeJxFW2SP4XRrFd1JYnxeLeU8EqN3vDfZmbqBqaGJAyiLjTAwm6ZLRQUMv1ZACTj37sR62cfN7fe5JnJ7dh8zL4fiyLHV"
            ),
            (
                "m/0'/1/2'/2/1000000000",
                "000102030405060708090a0b0c0d0e0f",
                "1000000000",
                "471b76e389e528d6de6d816857e012c5455051cad6660850e58372a6c3e6e7c8",
                "c783e67b921d2beb8f6b389cc646d7263b4145701dadd2161548a8b078e65e9e",
                "d880d7d8",
                "xprvA41z7zogVVwxVSgdKUHDy1SKmdb533PjDz7J6N6mV6uS3ze1ai8FHa8kmHScGpWmj4WggLyQjgPie1rFSruoUihUZREPSL39UNdE3BBDu76",
                "xpub6H1LXWLaKsWFhvm6RVpEL9P4KfRZSW7abD2ttkWP3SSQvnyA8FSVqNTEcYFgJS2UaFcxupHiYkro49S8yGasTvXEYBVPamhGW6cFJodrTHy"
            ),
            (
                "m",
                "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
                "0",
                "4b03d6fc340455b363f51020ad3ecca4f0850280cf436c70c727923f6db46c3e",
                "60499f801b896d83179a4374aeb7822aaeaceaa0db1f85ee3e904c4defbd9689",
                "00000000",
                "xprv9s21ZrQH143K31xYSDQpPDxsXRTUcvj2iNHm5NUtrGiGG5e2DtALGdso3pGz6ssrdK4PFmM8NSpSBHNqPqm55Qn3LqFtT2emdEXVYsCzC2U",
                "xpub661MyMwAqRbcFW31YEwpkMuc5THy2PSt5bDMsktWQcFF8syAmRUapSCGu8ED9W6oDMSgv6Zz8idoc4a6mr8BDzTJY47LJhkJ8UB7WEGuduB"
            ),
            (
                "m/0",
                "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
                "0",
                "abe74a98f6c7eabee0428f53798f0ab8aa1bd37873999041703c742f15ac7e1e",
                "f0909affaa7ee7abe5dd4e100598d4dc53cd709d5a5c2cac40e7412f232f7c9c",
                "bd16bee5",
                "xprv9vHkqa6EV4sPZHYqZznhT2NPtPCjKuDKGY38FBWLvgaDx45zo9WQRUT3dKYnjwih2yJD9mkrocEZXo1ex8G81dwSM1fwqWpWkeS3v86pgKt",
                "xpub69H7F5d8KSRgmmdJg2KhpAK8SR3DjMwAdkxj3ZuxV27CprR9LgpeyGmXUbC6wb7ERfvrnKZjXoUmmDznezpbZb7ap6r1D3tgFxHmwMkQTPH"
            ),
            (
                "m/0/2147483647'",
                "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
                "4294967295",
                "877c779ad9687164e9c2f4f0f4ff0340814392330693ce95a58fe18fd52e6e93",
                "be17a268474a6bb9c61e1d720cf6215e2a88c5406c4aee7b38547f585c9a37d9",
                "5a61ff8e",
                "xprv9wSp6B7kry3Vj9m1zSnLvN3xH8RdsPP1Mh7fAaR7aRLcQMKTR2vidYEeEg2mUCTAwCd6vnxVrcjfy2kRgVsFawNzmjuHc2YmYRmagcEPdU9",
                "xpub6ASAVgeehLbnwdqV6UKMHVzgqAG8Gr6riv3Fxxpj8ksbH9ebxaEyBLZ85ySDhKiLDBrQSARLq1uNRts8RuJiHjaDMBU4Zn9h8LZNnBC5y4a"
            ),
            (
                "m/0/2147483647'/1",
                "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
                "1",
                "704addf544a06e5ee4bea37098463c23613da32020d604506da8c0518e1da4b7",
                "f366f48f1ea9f2d1d3fe958c95ca84ea18e4c4ddb9366c336c927eb246fb38cb",
                "d8ab4937",
                "xprv9zFnWC6h2cLgpmSA46vutJzBcfJ8yaJGg8cX1e5StJh45BBciYTRXSd25UEPVuesF9yog62tGAQtHjXajPPdbRCHuWS6T8XA2ECKADdw4Ef",
                "xpub6DF8uhdarytz3FWdA8TvFSvvAh8dP3283MY7p2V4SeE2wyWmG5mg5EwVvmdMVCQcoNJxGoWaU9DCWh89LojfZ537wTfunKau47EL2dhHKon"
            ),
            (
                "m/0/2147483647'/1/2147483646'",
                "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
                "4294967294",
                "f1c7c871a54a804afe328b4c83a1c33b8e5ff48f5087273f04efa83b247d6a2d",
                "637807030d55d01f9a0cb3a7839515d796bd07706386a6eddf06cc29a65a0e29",
                "78412e3a",
                "xprvA1RpRA33e1JQ7ifknakTFpgNXPmW2YvmhqLQYMmrj4xJXXWYpDPS3xz7iAxn8L39njGVyuoseXzU6rcxFLJ8HFsTjSyQbLYnMpCqE2VbFWc",
                "xpub6ERApfZwUNrhLCkDtcHTcxd75RbzS1ed54G1LkBUHQVHQKqhMkhgbmJbZRkrgZw4koxb5JaHWkY4ALHY2grBGRjaDMzQLcgJvLJuZZvRcEL"
            ),
            (
                "m/0/2147483647'/1/2147483646'/2",
                "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
                "2",
                "bb7d39bdb83ecf58f2fd82b6d918341cbef428661ef01ab97c28a4842125ac23",
                "9452b549be8cea3ecb7a84bec10dcfd94afe4d129ebfd3b3cb58eedf394ed271",
                "31a507b8",
                "xprvA2nrNbFZABcdryreWet9Ea4LvTJcGsqrMzxHx98MMrotbir7yrKCEXw7nadnHM8Dq38EGfSh6dqA9QWTyefMLEcBYJUuekgW4BYPJcr9E7j",
                "xpub6FnCn6nSzZAw5Tw7cgR9bi15UV96gLZhjDstkXXxvCLsUXBGXPdSnLFbdpq8p9HmGsApME5hQTZ3emM2rnY5agb9rXpVGyy3bdW6EEgAtqt"
            ),
        ];

        #[test]
        fn new() {
            KEYPAIRS.iter().for_each(|(path, seed, child_index, secret_key, chain_code, parent_fingerprint, extended_private_key, _)| {
                test_new::<N>(
                    extended_private_key,
                    parent_fingerprint,
                    child_index.parse().unwrap(),
                    chain_code,
                    secret_key,
                    seed,
                    &Format::P2PKH,
                    &BitcoinDerivationPath::from_str(path).unwrap());
            });
        }

        #[test]
        fn derive() {
            KEYPAIRS.chunks(2).for_each(|pair| {
                let (_, _, _, _, _, _, expected_extended_private_key1, _) = pair[0];
                let (_, _, expected_child_index2, _, _, _, expected_extended_private_key2, _) = pair[1];
                test_derive::<N>(
                    expected_extended_private_key1,
                    expected_extended_private_key2,
                    expected_child_index2.parse().unwrap(),
                );
            });
        }

        #[test]
        fn to_extended_public_key() {
            KEYPAIRS.iter().for_each(|(path, seed, _, _, _, _, _, expected_public_key)| {
                test_to_extended_public_key::<N>(
                    expected_public_key,
                    seed,
                    &Format::P2PKH,
                    &BitcoinDerivationPath::from_str(path).unwrap());
            });
        }

        #[test]
        fn from_str() {
            KEYPAIRS.iter().for_each(|(_, _, child_index, secret_key, chain_code, parent_fingerprint, extended_private_key, _)| {
                test_from_str::<N>(
                    extended_private_key,
                    parent_fingerprint,
                    child_index.parse().unwrap(),
                    chain_code,
                    secret_key);
            });
        }

        #[test]
        fn to_string() {
            KEYPAIRS.iter().for_each(|(_, _, _, _, _, _, extended_private_key, _)| {
                test_to_string::<N>(extended_private_key);
            });
        }
    }

    mod bip44 {
        use super::*;

        #[test]
        fn test_derivation_path() {
            type N = Mainnet;
            let path = "m/44'/0'/0/1";
            let expected_xpriv_serialized = "xprvA1ErCzsuXhpB8iDTsbmgpkA2P8ggu97hMZbAXTZCdGYeaUrDhyR8fEw47BNEgLExsWCVzFYuGyeDZJLiFJ9kwBzGojQ6NB718tjVJrVBSrG";
            let master_xpriv = BitcoinExtendedPrivateKey::<N>::from_str("xprv9s21ZrQH143K4KqQx9Zrf1eN8EaPQVFxM2Ast8mdHn7GKiDWzNEyNdduJhWXToy8MpkGcKjxeFWd8oBSvsz4PCYamxR7TX49pSpp3bmHVAY").unwrap();
            let xpriv = master_xpriv.derive(&BitcoinDerivationPath::from_str(path).unwrap()).unwrap();
            assert_eq!(expected_xpriv_serialized, xpriv.to_string());
        }
    }

//    mod bip49 {
//        use super::*;
//
//        #[test]
//        fn test_bip49() {
//            let seed = "tprv8ZgxMBicQKsPe5YMU9gHen4Ez3ApihUfykaqUorj9t6FDqy3nP6eoXiAo2ssvpAjoLroQxHqr3R5nE3a5dU3DHTjTgJDd7zrbniJr6nrCzd";
//            let seed_bytes = hex::decode(seed).expect("Error decoding hex seed");
//            let master_xpriv = BitcoinExtendedPrivateKey::new(&seed_bytes, &Network::Mainnet);
//            let root_path = "m/49'/1'/0'";
//            let expected_xpriv_serialized = "tprv8gRrNu65W2Msef2BdBSUgFdRTGzC8EwVXnV7UGS3faeXtuMVtGfEdidVeGbThs4ELEoayCAzZQ4uUji9DUiAs7erdVskqju7hrBcDvDsdbY";
//            let root_xpriv = master_xpriv.derivation_path(&root_path);
//            assert_eq!(root_xpriv.to_string(), expected_xpriv_serialized);
//
//            let account_path = "m/49'/1'/0'/0/0";
//            let expected_private_key = "cULrpoZGXiuC19Uhvykx7NugygA3k86b3hmdCeyvHYQZSxojGyXJ";
//            let account_xpriv = master_xpriv.derivation_path(&account_path);
//            assert_eq!(account_xpriv.private_key.to_string(), expected_private_key);
//        }
//    }

    mod test_invalid {
        use super::*;

        type N = Mainnet;

        const INVALID_XPRIV_SECRET_KEY: &str = "xprv9s21ZrQH143K24Mfq5zL5MhWK9hUhhGbd45hLXo2Pq2oqzMMo63oStZzFAzHGBP2UuGCqWLTAPLcMtD9y5gkZ6Eq3Rjuahrv17fENZ3QzxW";
        const INVALID_XPRIV_NETWORK: &str = "xprv8s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";
        const INVALID_XPRIV_CHECKSUM: &str = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHj";
        const VALID_XPRIV: &str = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";

        #[test]
        #[should_panic(expected = "Crate(\"secp256k1\", \"InvalidSecretKey\")")]
        fn from_str_invalid_secret_key() {
            let _result = BitcoinExtendedPrivateKey::<N>::from_str(INVALID_XPRIV_SECRET_KEY).unwrap();
        }

        #[test]
        #[should_panic(expected = "InvalidVersionBytes([4, 136, 173, 227])")]
        fn from_str_invalid_version() {
            let _result = BitcoinExtendedPrivateKey::<N>::from_str(INVALID_XPRIV_NETWORK).unwrap();
        }

        #[test]
        #[should_panic(expected = "InvalidChecksum(\"6vCfku\", \"6vCfkt\")")]
        fn from_str_invalid_checksum() {
            let _result = BitcoinExtendedPrivateKey::<N>::from_str(INVALID_XPRIV_CHECKSUM).unwrap();
        }

        #[test]
        #[should_panic(expected = "InvalidByteLength(81)")]
        fn from_str_short() {
            let _result = BitcoinExtendedPrivateKey::<N>::from_str(&VALID_XPRIV[1..]).unwrap();
        }

        #[test]
        #[should_panic(expected = "InvalidByteLength(83)")]
        fn from_str_long() {
            let mut string = String::from(VALID_XPRIV);
            string.push('a');
            let _result = BitcoinExtendedPrivateKey::<N>::from_str(&string).unwrap();
        }
    }
}