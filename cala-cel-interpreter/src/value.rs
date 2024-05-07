use cel_parser::{ast::Literal, Expression};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

use std::{collections::HashMap, sync::Arc};

use crate::{cel_type::*, error::*};

pub struct CelResult<'a> {
    pub expr: &'a Expression,
    pub val: CelValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CelValue {
    // Builtins
    Map(Arc<CelMap>),
    List(Arc<CelArray>),
    Int(i64),
    UInt(u64),
    Double(f64),
    String(Arc<String>),
    Bytes(Arc<Vec<u8>>),
    Bool(bool),
    Null,

    // Addons
    Decimal(Decimal),
    Date(NaiveDate),
    Uuid(Uuid),
}

impl CelValue {
    pub(crate) fn try_bool(&self) -> Result<bool, CelError> {
        if let CelValue::Bool(val) = self {
            Ok(*val)
        } else {
            Err(CelError::BadType(CelType::Bool, CelType::from(self)))
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct CelMap {
    inner: HashMap<CelKey, CelValue>,
}

#[derive(Debug, PartialEq)]
pub struct CelArray {
    inner: Vec<CelValue>,
}

impl CelArray {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn push(&mut self, elem: impl Into<CelValue>) {
        self.inner.push(elem.into());
    }
}

impl Default for CelArray {
    fn default() -> Self {
        Self::new()
    }
}

impl CelMap {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, k: impl Into<CelKey>, val: impl Into<CelValue>) {
        self.inner.insert(k.into(), val.into());
    }

    pub fn get(&self, key: impl Into<CelKey>) -> CelValue {
        self.inner
            .get(&key.into())
            .cloned()
            .unwrap_or(CelValue::Null)
    }
}

impl Default for CelMap {
    fn default() -> Self {
        Self::new()
    }
}

impl From<HashMap<String, CelValue>> for CelMap {
    fn from(map: HashMap<String, CelValue>) -> Self {
        let mut res = CelMap::new();
        for (k, v) in map {
            res.insert(CelKey::String(Arc::from(k)), v);
        }
        res
    }
}

impl From<CelMap> for CelValue {
    fn from(m: CelMap) -> Self {
        CelValue::Map(Arc::from(m))
    }
}

impl From<i64> for CelValue {
    fn from(i: i64) -> Self {
        CelValue::Int(i)
    }
}

impl From<Decimal> for CelValue {
    fn from(d: Decimal) -> Self {
        CelValue::Decimal(d)
    }
}

impl From<bool> for CelValue {
    fn from(b: bool) -> Self {
        CelValue::Bool(b)
    }
}

impl From<String> for CelValue {
    fn from(s: String) -> Self {
        CelValue::String(Arc::from(s))
    }
}

impl From<NaiveDate> for CelValue {
    fn from(d: NaiveDate) -> Self {
        CelValue::Date(d)
    }
}

impl From<Uuid> for CelValue {
    fn from(id: Uuid) -> Self {
        CelValue::Uuid(id)
    }
}

impl From<&str> for CelValue {
    fn from(s: &str) -> Self {
        CelValue::String(Arc::from(s.to_string()))
    }
}

impl From<serde_json::Value> for CelValue {
    fn from(v: serde_json::Value) -> Self {
        use serde_json::Value::*;
        match v {
            Null => CelValue::Null,
            Bool(b) => CelValue::Bool(b),
            Number(n) => {
                if let Some(u) = n.as_u64() {
                    CelValue::UInt(u)
                } else if let Some(i) = n.as_i64() {
                    CelValue::Int(i)
                } else {
                    unimplemented!()
                }
            }
            String(s) => CelValue::String(Arc::from(s)),
            Object(o) => {
                let mut map = CelMap::new();
                for (k, v) in o.into_iter() {
                    map.insert(CelKey::String(Arc::from(k)), CelValue::from(v));
                }
                CelValue::Map(Arc::from(map))
            }
            Array(a) => {
                let mut ar = CelArray::new();
                for v in a.into_iter() {
                    ar.push(CelValue::from(v));
                }
                CelValue::List(Arc::from(ar))
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum CelKey {
    Int(i64),
    UInt(u64),
    Bool(bool),
    String(Arc<String>),
}

impl From<&str> for CelKey {
    fn from(s: &str) -> Self {
        CelKey::String(Arc::from(s.to_string()))
    }
}

impl From<String> for CelKey {
    fn from(s: String) -> Self {
        CelKey::String(Arc::from(s))
    }
}

impl From<&Arc<String>> for CelKey {
    fn from(s: &Arc<String>) -> Self {
        CelKey::String(s.clone())
    }
}

impl From<&CelValue> for CelType {
    fn from(v: &CelValue) -> Self {
        match v {
            CelValue::Map(_) => CelType::Map,
            CelValue::List(_) => CelType::List,
            CelValue::Int(_) => CelType::Int,
            CelValue::UInt(_) => CelType::UInt,
            CelValue::Double(_) => CelType::Double,
            CelValue::String(_) => CelType::String,
            CelValue::Bytes(_) => CelType::Bytes,
            CelValue::Bool(_) => CelType::Bool,
            CelValue::Null => CelType::Null,

            CelValue::Decimal(_) => CelType::Decimal,
            CelValue::Date(_) => CelType::Date,
            CelValue::Uuid(_) => CelType::Uuid,
        }
    }
}

impl From<&Literal> for CelValue {
    fn from(l: &Literal) -> Self {
        use Literal::*;
        match l {
            Int(i) => CelValue::Int(*i),
            UInt(u) => CelValue::UInt(*u),
            Double(d) => CelValue::Double(d.parse().expect("Couldn't parse Decimal")),
            String(s) => CelValue::String(s.clone()),
            Bytes(b) => CelValue::Bytes(b.clone()),
            Bool(b) => CelValue::Bool(*b),
            Null => CelValue::Null,
        }
    }
}

impl TryFrom<&CelValue> for Arc<String> {
    type Error = CelError;

    fn try_from(v: &CelValue) -> Result<Self, Self::Error> {
        if let CelValue::String(s) = v {
            Ok(s.clone())
        } else {
            Err(CelError::BadType(CelType::String, CelType::from(v)))
        }
    }
}

impl<'a> TryFrom<CelResult<'a>> for NaiveDate {
    type Error = CelError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::Date(d) = val {
            Ok(d)
        } else {
            Err(CelError::EvaluationError(
                format!("{expr:?}"),
                Box::new(CelError::BadType(CelType::Date, CelType::from(&val))),
            ))
        }
    }
}

impl<'a> TryFrom<CelResult<'a>> for Uuid {
    type Error = CelError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::Uuid(id) = val {
            Ok(id)
        } else {
            Err(CelError::EvaluationError(
                format!("{expr:?}"),
                Box::new(CelError::BadType(CelType::Uuid, CelType::from(&val))),
            ))
        }
    }
}

impl<'a> TryFrom<CelResult<'a>> for String {
    type Error = CelError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::String(s) = val {
            Ok(s.to_string())
        } else {
            Err(CelError::EvaluationError(
                format!("{expr:?}"),
                Box::new(CelError::BadType(CelType::String, CelType::from(&val))),
            ))
        }
    }
}

impl<'a> TryFrom<CelResult<'a>> for Decimal {
    type Error = CelError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        match val {
            CelValue::Decimal(n) => Ok(n),
            _ => Err(CelError::EvaluationError(
                format!("{expr:?}"),
                Box::new(CelError::BadType(CelType::Decimal, CelType::from(&val))),
            )),
        }
    }
}

impl From<&CelKey> for CelType {
    fn from(v: &CelKey) -> Self {
        match v {
            CelKey::Int(_) => CelType::Int,
            CelKey::UInt(_) => CelType::UInt,
            CelKey::Bool(_) => CelType::Bool,
            CelKey::String(_) => CelType::String,
        }
    }
}

impl TryFrom<&CelKey> for String {
    type Error = CelError;

    fn try_from(v: &CelKey) -> Result<Self, Self::Error> {
        if let CelKey::String(s) = v {
            Ok(s.to_string())
        } else {
            Err(CelError::BadType(CelType::String, CelType::from(v)))
        }
    }
}

impl<'a> TryFrom<CelResult<'a>> for serde_json::Value {
    type Error = CelError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        use serde_json::*;
        Ok(match val {
            CelValue::Int(n) => Value::from(n),
            CelValue::UInt(n) => Value::from(n),
            CelValue::Double(n) => Value::from(n.to_string()),
            CelValue::Bool(b) => Value::from(b),
            CelValue::String(n) => Value::from(n.as_str()),
            CelValue::Null => Value::Null,
            CelValue::Date(d) => Value::from(d.to_string()),
            CelValue::Uuid(u) => Value::from(u.to_string()),
            CelValue::Map(m) => {
                let mut res = serde_json::Map::new();
                for (k, v) in m.inner.iter() {
                    let key: String = k.try_into()?;
                    let value = Self::try_from(CelResult {
                        expr,
                        val: v.clone(),
                    })?;
                    res.insert(key, value);
                }
                Value::from(res)
            }
            CelValue::List(a) => {
                let mut res = Vec::new();
                for v in a.inner.iter() {
                    res.push(Self::try_from(CelResult {
                        expr,
                        val: v.clone(),
                    })?);
                }
                Value::from(res)
            }
            _ => unimplemented!(),
        })
    }
}
