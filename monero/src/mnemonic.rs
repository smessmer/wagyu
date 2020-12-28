use crate::address::MoneroAddress;
use crate::format::MoneroFormat;
use crate::network::MoneroNetwork;
use crate::private_key::MoneroPrivateKey;
use crate::public_key::MoneroPublicKey;
use crate::wordlist::MoneroWordlist;
use wagyu_model::{Mnemonic, MnemonicError, PrivateKey};

use crc::{crc32, Hasher32};
use curve25519_dalek::scalar::Scalar;
use rand::Rng;
use std::{fmt, marker::PhantomData, str, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Represents a Monero mnemonic
pub struct MoneroMnemonic<N: MoneroNetwork, W: MoneroWordlist> {
    /// The initial 256-bit seed
    seed: [u8; 32],
    /// PhantomData
    _network: PhantomData<N>,
    /// PhantomData
    _wordlist: PhantomData<W>,
}

impl<N: MoneroNetwork, W: MoneroWordlist> Mnemonic for MoneroMnemonic<N, W> {
    type Address = MoneroAddress<N>;
    type Format = MoneroFormat;
    type PrivateKey = MoneroPrivateKey<N>;
    type PublicKey = MoneroPublicKey<N>;

    /// Returns a new mnemonic.
    fn new<R: Rng>(rng: &mut R) -> Result<Self, MnemonicError> {
        Ok(Self {
            seed: rng.gen(),
            _network: PhantomData,
            _wordlist: PhantomData,
        })
    }

    /// Returns the mnemonic for the given phrase.
    fn from_phrase(phrase: &str) -> Result<Self, MnemonicError> {
        let length = 1626;
        let words = phrase.split(" ").collect::<Vec<&str>>();
        let mut phrase = words.iter().map(|word| word.to_string()).collect::<Vec<String>>();

        if phrase.len() % 3 == 2 {
            return Err(MnemonicError::MissingWord);
        } else if phrase.len() % 3 == 0 {
            return Err(MnemonicError::MissingChecksumWord);
        }

        let checksum = match phrase.pop() {
            Some(word) => word,
            _ => return Err(MnemonicError::MissingWord),
        };

        // Decode the phrase
        let mut buffer = vec![];
        let chunks = phrase.chunks(3);
        for chunk in chunks {
            let w1 = W::get_index_trimmed(&W::to_trimmed(&chunk[0]))?;
            let w2 = W::get_index_trimmed(&W::to_trimmed(&chunk[1]))?;
            let w3 = W::get_index_trimmed(&W::to_trimmed(&chunk[2]))?;

            let n = length;
            let x = w1 + n * (((n - w1) + w2) % n) + n * n * (((n - w2) + w3) % n);

            if x % n != w1 {
                return Err(MnemonicError::InvalidDecoding);
            }

            buffer.extend_from_slice(&u32::to_le_bytes(x as u32));
        }

        // Verify the checksum
        let expected_checksum = Self::checksum_word(&phrase.into());
        if W::to_trimmed(&expected_checksum) != W::to_trimmed(&checksum) {
            let expected = W::to_trimmed(&expected_checksum);
            let found = W::to_trimmed(&checksum);
            return Err(MnemonicError::InvalidChecksumWord(expected, found));
        }

        let mut seed = [0u8; 32];
        seed.copy_from_slice(&buffer);

        Ok(Self {
            seed,
            _network: PhantomData,
            _wordlist: PhantomData,
        })
    }

    fn to_phrase(&self) -> Result<String, MnemonicError> {
        let seed = &Scalar::from_bytes_mod_order(self.seed).to_bytes();

        // Reverse the endian in 4 byte intervals
        let length = 1626;
        let inputs = seed
            .chunks(4)
            .map(|chunk| {
                let mut input: [u8; 4] = [0u8; 4];
                input.copy_from_slice(chunk);

                u32::from_le_bytes(input)
            })
            .collect::<Vec<u32>>();

        // Generate three words from every 4 byte interval
        let mut phrase = vec![];
        for index in inputs {
            let w1 = index % length;
            let w2 = ((index / length) + w1) % length;
            let w3 = (((index / length) / length) + w2) % length;

            phrase.push(W::get(w1 as usize)?);
            phrase.push(W::get(w2 as usize)?);
            phrase.push(W::get(w3 as usize)?);
        }

        // Compute the checksum word
        phrase.push(Self::checksum_word(&phrase));

        Ok(phrase.join(" "))
    }

    /// Returns the private key of the corresponding mnemonic.
    fn to_private_key(&self, _: Option<&str>) -> Result<Self::PrivateKey, MnemonicError> {
        Ok(MoneroPrivateKey::from_seed(
            hex::encode(&self.seed).as_str(),
            &MoneroFormat::Standard,
        )?)
    }

    /// Returns the public key of the corresponding mnemonic.
    fn to_public_key(&self, _: Option<&str>) -> Result<Self::PublicKey, MnemonicError> {
        Ok(self.to_private_key(None)?.to_public_key())
    }

    /// Returns the address of the corresponding mnemonic.
    fn to_address(&self, _: Option<&str>, _: &Self::Format) -> Result<Self::Address, MnemonicError> {
        Ok(self.to_private_key(None)?.to_address(&MoneroFormat::Standard)?)
    }

    /// Returns the seed entropy of the corresponding mnemonic.
    fn entropy(&self) -> &[u8] {
        &self.seed
    }
}

impl<N: MoneroNetwork, W: MoneroWordlist> MoneroMnemonic<N, W> {
    /// Returns the mnemonic of the given private spend key
    pub fn from_private_spend_key(private_spend_key: &[u8; 32]) -> Self {
        Self {
            seed: *private_spend_key,
            _network: PhantomData,
            _wordlist: PhantomData,
        }
    }

    /// Compares the given phrase against the phrase extracted from its entropy.
    pub fn verify_phrase(phrase: &str) -> bool {
        Self::from_phrase(phrase).is_ok()
    }

    /// Returns the checksum word for a given phrase.
    fn checksum_word(phrase: &Vec<String>) -> String {
        let phrase_trimmed = phrase.iter().map(|word| W::to_trimmed(word)).collect::<Vec<String>>();

        let mut digest = crc32::Digest::new(crc32::IEEE);
        digest.write(phrase_trimmed.concat().as_bytes());
        phrase[(digest.sum32() % phrase.len() as u32) as usize].clone()
    }
}

impl<N: MoneroNetwork, W: MoneroWordlist> FromStr for MoneroMnemonic<N, W> {
    type Err = MnemonicError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_phrase(s)
    }
}

impl<N: MoneroNetwork, W: MoneroWordlist> fmt::Display for MoneroMnemonic<N, W> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self.to_phrase() {
                Ok(phrase) => phrase,
                _ => return Err(fmt::Error),
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::*;
    use crate::wordlist::*;
    use hex;

    fn test_new<N: MoneroNetwork, W: MoneroWordlist>() {
        let rng = &mut rand::thread_rng();
        let mnemonic = MoneroMnemonic::<N, W>::new(rng).unwrap();
        test_to_phrase::<N, W>(&mnemonic.to_phrase().unwrap(), &mnemonic.seed);
    }

    fn test_from_phrase<N: MoneroNetwork, W: MoneroWordlist>(expected_seed: &[u8; 32], phrase: &str) {
        let mnemonic = MoneroMnemonic::<N, W>::from_phrase(phrase).unwrap();
        assert_eq!(&expected_seed[..], &mnemonic.seed[..]);
        assert_eq!(phrase, mnemonic.to_phrase().unwrap());
    }

    fn test_to_phrase<N: MoneroNetwork, W: MoneroWordlist>(expected_phrase: &str, seed: &[u8; 32]) {
        let mnemonic = MoneroMnemonic::<N, W> {
            seed: *seed,
            _network: PhantomData,
            _wordlist: PhantomData,
        };
        assert_eq!(&seed[..], &mnemonic.seed[..]);
        assert_eq!(expected_phrase, mnemonic.to_phrase().unwrap());
    }

    fn test_verify_phrase<N: MoneroNetwork, W: MoneroWordlist>(phrase: &str) {
        assert!(MoneroMnemonic::<N, W>::verify_phrase(phrase));
    }

    fn test_to_private_key<N: MoneroNetwork, W: MoneroWordlist>(
        expected_private_spend_key: &str,
        expected_private_view_key: &str,
        phrase: &str,
    ) {
        let mnemonic = MoneroMnemonic::<N, W>::from_phrase(phrase).unwrap();
        let private_key = mnemonic.to_private_key(None).unwrap();
        assert_eq!(
            expected_private_spend_key,
            hex::encode(private_key.to_private_spend_key())
        );
        assert_eq!(
            expected_private_view_key,
            hex::encode(private_key.to_private_view_key())
        );
    }

    mod english {
        use super::*;

        type N = Mainnet;
        type W = English;

        // (seed, phrase, (private_spend_key, private_view_key))
        const KEYPAIRS: [(&str, &str, (&str, &str)); 5] = [
            (
                "82a13b87b69555ba976601302e2498aed4875185c87b9133bf8d214f16e9eb0b",
                "reruns today hookup itself thorn nirvana symptoms jukebox patio unquoted sushi long diode digit rewind hacksaw obvious soothe nightly return agile hobby algebra awesome nirvana",
                ("82a13b87b69555ba976601302e2498aed4875185c87b9133bf8d214f16e9eb0b", "5ea51b4da3e87ded053383ca38945d38c3bb35d6b84bf7a1c45b2a4f713f8705")
            ),
            (
                "31e28ef4feca46915bdbf7b192af866e154cb7dbc704e9a39b6ce24ac89c1102",
                "cafe aided wounded lumber hounded water yoyo gasp aerial merger ungainly gaze ruby yacht tell playful smash issued sifting whole erase anxiety dash deity sifting",
                ("31e28ef4feca46915bdbf7b192af866e154cb7dbc704e9a39b6ce24ac89c1102", "68cef3455e6967a9751959914c3cbc5d990cafa07fb65be15c5478d17abe8a02")
            ),
            (
                "ea111187a598d5ab5fdabf8adb27df79005a106c7e3dc11797d77c4c48bace0b",
                "fight hoisting uptight nibs womanly pepper does plotting dolphin fugitive popular chlorine turnip organs ambush people hospital ledge puppy anybody gourmet cuddled because candy womanly",
                ("ea111187a598d5ab5fdabf8adb27df79005a106c7e3dc11797d77c4c48bace0b", "66dac59f937a9883ce6c12a515755bb744079d1a456a08964e92fc2a0748630a")
            ),
            (
                "a6e0194a91f45a4f08633efc405e63d7c509d926759e7a9b7b945f235a8d300e",
                "roped waist elapse cider reruns aggravate jetting bested azure omnibus hull economics depth reheat tobacco exit under locker money actress certain cupcake drinks examine reheat",
                ("a6e0194a91f45a4f08633efc405e63d7c509d926759e7a9b7b945f235a8d300e", "09e13dda6b81a3d739f6714bed246071dc184dde0cb3edc71b5a984b1b67f003")
            ),
            (
                "09ec1221eee3d94452d688e8894c0917b73d14dbcda3ef673b038a0874e5ee02",
                "pigment mice pitched examine damp jobs going viewpoint terminal ultimate asylum cogs saved wayside stylishly asylum opposite after ghetto malady mural uphill maps metro pigment",
                ("09ec1221eee3d94452d688e8894c0917b73d14dbcda3ef673b038a0874e5ee02", "9a669bdaa1a4f2de752435db6eead238ff3c191797e0a86515b85e880c7bda01")
            )
        ];

        #[test]
        fn new() {
            (0..10).for_each(|_| test_new::<N, W>())
        }

        #[test]
        fn from_phrase() {
            KEYPAIRS.iter().for_each(|(seed, phrase, _)| {
                let mut expected_seed = [0u8; 32];
                expected_seed.copy_from_slice(&hex::decode(seed).unwrap());
                test_from_phrase::<N, W>(&expected_seed, phrase);
            })
        }

        #[test]
        fn to_phrase() {
            KEYPAIRS.iter().for_each(|(seed_str, expected_phrase, _)| {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&hex::decode(seed_str).unwrap());
                test_to_phrase::<N, W>(expected_phrase, &seed);
            });
        }

        #[test]
        fn verify_phrase() {
            KEYPAIRS.iter().for_each(|(_, phrase, _)| {
                test_verify_phrase::<N, W>(phrase);
            });
        }

        #[test]
        fn to_private_key() {
            KEYPAIRS
                .iter()
                .for_each(|(_, phrase, (expected_private_spend_key, expected_private_view_key))| {
                    test_to_private_key::<N, W>(expected_private_spend_key, expected_private_view_key, phrase);
                });
        }
    }
}
