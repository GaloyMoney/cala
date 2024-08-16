use thiserror::Error;

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("IntegrationError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("XPubError - Serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("XPubError - FromHex: {0}")]
    FromHex(#[from] hex::FromHexError),
    #[error("Could not decrypt signer config: {0}")]
    DecryptionError(chacha20poly1305::Error),
}
