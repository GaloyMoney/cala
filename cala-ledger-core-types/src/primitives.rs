use rusty_money::{crypto, iso};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use cel_interpreter::{CelResult, CelType, CelValue, ResultCoercionError};

crate::entity_id! { OutboxEventId }
crate::entity_id! { AccountId }
crate::entity_id! { AccountSetId }
crate::entity_id! { JournalId }
crate::entity_id! { DataSourceId }
crate::entity_id! { TxTemplateId }
crate::entity_id! { TransactionId }
crate::entity_id! { EntryId }

pub type BalanceId = (JournalId, AccountId, Currency);

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "DebitOrCredit", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum DebitOrCredit {
    Debit,
    Credit,
}

impl Default for DebitOrCredit {
    fn default() -> Self {
        Self::Credit
    }
}

impl<'a> TryFrom<CelResult<'a>> for DebitOrCredit {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        match val {
            CelValue::String(v) if v.as_ref() == "DEBIT" => Ok(DebitOrCredit::Debit),
            CelValue::String(v) if v.as_ref() == "CREDIT" => Ok(DebitOrCredit::Credit),
            v => Err(ResultCoercionError::BadExternalTypeCoercion(
                format!("{expr:?}"),
                CelType::from(&v),
                "DebitOrCredit",
            )),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "Status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Locked,
}

impl Default for Status {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type)]
#[sqlx(type_name = "Layer", rename_all = "snake_case")]
pub enum Layer {
    Settled,
    Pending,
    Encumbered,
}

#[derive(thiserror::Error, Debug)]
pub enum ParseLayerError {
    #[error("CalaCoreTypeError - UnknownLayer: {0:?}")]
    UnknownLayer(String),
}

impl<'a> TryFrom<CelResult<'a>> for Layer {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        match val {
            CelValue::String(v) if v.as_ref() == "SETTLED" => Ok(Layer::Settled),
            CelValue::String(v) if v.as_ref() == "PENDING" => Ok(Layer::Pending),
            CelValue::String(v) if v.as_ref() == "ENCUMBERED" => Ok(Layer::Encumbered),
            v => Err(ResultCoercionError::BadExternalTypeCoercion(
                format!("{expr:?}"),
                CelType::from(&v),
                "Layer",
            )),
        }
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self::Settled
    }
}

#[derive(Debug, Clone, Copy, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "&str")]
pub enum Currency {
    Iso(&'static iso::Currency),
    Crypto(&'static crypto::Currency),
}

impl Currency {
    pub fn code(&self) -> &'static str {
        match self {
            Currency::Iso(c) => c.iso_alpha_code,
            Currency::Crypto(c) => c.code,
        }
    }
}

impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

impl std::hash::Hash for Currency {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.code().hash(state);
    }
}

impl PartialEq for Currency {
    fn eq(&self, other: &Self) -> bool {
        self.code() == other.code()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseCurrencyError {
    #[error("CalaCoreTypeError - UnknownCurrency: {0}")]
    UnknownCurrency(String),
}

impl std::str::FromStr for Currency {
    type Err = ParseCurrencyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match iso::find(s) {
            Some(c) => Ok(Currency::Iso(c)),
            _ => match crypto::find(s) {
                Some(c) => Ok(Currency::Crypto(c)),
                _ => Err(ParseCurrencyError::UnknownCurrency(s.to_string())),
            },
        }
    }
}

impl TryFrom<String> for Currency {
    type Error = ParseCurrencyError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Currency> for &'static str {
    fn from(c: Currency) -> Self {
        c.code()
    }
}

impl<'a> TryFrom<CelResult<'a>> for Currency {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        match val {
            CelValue::String(v) => v.as_ref().parse::<Currency>().map_err(|e| {
                ResultCoercionError::ExternalTypeCoercionError(
                    format!("{expr:?}"),
                    format!("{v:?}"),
                    "Currency",
                    format!("{e:?}"),
                )
            }),
            v => Err(ResultCoercionError::BadExternalTypeCoercion(
                format!("{expr:?}"),
                CelType::from(&v),
                "Currency",
            )),
        }
    }
}

const _LOCAL_UUID: Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000000");

#[derive(Debug, Copy, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataSource {
    Local,
    Remote { id: DataSourceId },
}

impl From<DataSource> for Option<DataSourceId> {
    fn from(source: DataSource) -> Self {
        match source {
            DataSource::Local => None,
            DataSource::Remote { id } => Some(id),
        }
    }
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSource::Local => write!(f, "00000000-0000-0000-0000-000000000000"),
            DataSource::Remote { id } => write!(f, "{}", id),
        }
    }
}

impl std::str::FromStr for DataSource {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "00000000-0000-0000-0000-000000000000" {
            Ok(DataSource::Local)
        } else {
            Ok(DataSource::Remote { id: s.parse()? })
        }
    }
}
