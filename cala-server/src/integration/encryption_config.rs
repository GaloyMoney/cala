use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{error::IntegrationError, Integration};

pub type EncryptionKey = chacha20poly1305::Key;
#[derive(Clone)]
pub struct ConfigCipher(pub(super) Vec<u8>);
#[derive(Clone)]
pub struct Nonce(pub(super) Vec<u8>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(into = "RawEncryptionConfig")]
#[serde(try_from = "RawEncryptionConfig")]
pub struct EncryptionConfig {
    pub key: EncryptionKey,
}

impl Integration {
    pub(super) fn encrypt(
        &self,
        key: &EncryptionKey,
    ) -> Result<(ConfigCipher, Nonce), IntegrationError> {
        let cipher = ChaCha20Poly1305::new(key);
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let encrypted_config = cipher
            .encrypt(&nonce, serde_json::to_vec(&self.data)?.as_slice())
            .expect("should always encrypt");
        Ok((ConfigCipher(encrypted_config), Nonce(nonce.to_vec())))
    }

    pub(super) fn decrypt<T: DeserializeOwned>(
        encrypted_data: &ConfigCipher,
        nonce: &Nonce,
        key: &EncryptionKey,
    ) -> Result<T, IntegrationError> {
        let cipher = ChaCha20Poly1305::new(key);
        let decrypted_data = cipher
            .decrypt(
                chacha20poly1305::Nonce::from_slice(nonce.0.as_slice()),
                encrypted_data.0.as_slice(),
            )
            .map_err(IntegrationError::DecryptionError)?;
        let data = serde_json::from_slice(&decrypted_data)?;
        Ok(data)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct RawEncryptionConfig {
    pub key: String,
}
impl From<EncryptionConfig> for RawEncryptionConfig {
    fn from(config: EncryptionConfig) -> Self {
        Self {
            key: hex::encode(config.key),
        }
    }
}

impl TryFrom<RawEncryptionConfig> for EncryptionConfig {
    type Error = IntegrationError;

    fn try_from(raw: RawEncryptionConfig) -> Result<Self, Self::Error> {
        let key_vec = hex::decode(raw.key)?;
        let key_bytes = key_vec.as_slice();
        Ok(Self {
            key: EncryptionKey::clone_from_slice(key_bytes),
        })
    }
}

impl std::fmt::Debug for EncryptionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EncryptionConfig {{ key: *******Redacted******* }}")
    }
}

#[cfg(test)]
mod tests {
    pub use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Data {
        pub name: String,
        pub secret: String,
    }

    impl Default for Data {
        fn default() -> Self {
            Self {
                name: "Alice".to_string(),
                secret: "secret".to_string(),
            }
        }
    }

    fn gen_encryption_key() -> EncryptionKey {
        ChaCha20Poly1305::generate_key(&mut OsRng)
    }

    #[test]
    fn encrypt_decrypt() {
        let integration = Integration::new("name".to_string(), Data::default());
        let key = gen_encryption_key();
        let (encrypted, nonce) = integration.encrypt(&key).expect("Failed to encrypt");
        let decrypted: Data =
            Integration::decrypt(&encrypted, &nonce, &key).expect("Failed to decrypt");

        assert_eq!(integration.data, serde_json::to_value(&decrypted).unwrap());
    }
}
