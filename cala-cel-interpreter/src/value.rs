use cel::objects::{Key, Map, Opaque, Value};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use std::{collections::HashMap, sync::Arc};

use crate::{cel_type::*, error::*};

pub struct CelResult<'a> {
    pub expr: &'a str,
    pub val: CelValue,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CelDecimal(pub Decimal);

impl Opaque for CelDecimal {
    fn runtime_type_name(&self) -> &str {
        "cala.Decimal"
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CelUuid(pub Uuid);

impl Opaque for CelUuid {
    fn runtime_type_name(&self) -> &str {
        "cala.Uuid"
    }
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

    // Abstract
    Decimal(Decimal),
    Date(NaiveDate),
    Timestamp(DateTime<Utc>),
    Uuid(Uuid),
}

#[derive(Debug, PartialEq)]
pub struct CelMap {
    inner: HashMap<CelKey, CelValue>,
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

    pub fn contains_key(&self, key: impl Into<CelKey>) -> bool {
        self.inner.contains_key(&key.into())
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

impl CelValue {
    pub(crate) fn into_cel_value(self) -> Value {
        match self {
            CelValue::Map(map) => {
                let map = map
                    .inner
                    .iter()
                    .map(|(k, v)| (k.clone().into_cel_key(), v.clone().into_cel_value()))
                    .collect::<HashMap<_, _>>();
                Value::Map(Map { map: Arc::new(map) })
            }
            CelValue::List(array) => Value::List(Arc::new(
                array
                    .inner
                    .iter()
                    .map(|v| v.clone().into_cel_value())
                    .collect(),
            )),
            CelValue::Int(i) => Value::Int(i),
            CelValue::UInt(u) => Value::UInt(u),
            CelValue::Double(d) => Value::Float(d),
            CelValue::String(s) => Value::String(s),
            CelValue::Bytes(b) => Value::Bytes(b),
            CelValue::Bool(b) => Value::Bool(b),
            CelValue::Null => Value::Null,
            CelValue::Decimal(d) => Value::Opaque(Arc::new(CelDecimal(d))),
            CelValue::Date(d) => {
                let dt = d.and_hms_opt(0, 0, 0).expect("midnight is valid");
                Value::Timestamp(
                    FixedOffset::east_opt(0)
                        .expect("UTC offset is valid")
                        .from_utc_datetime(&dt),
                )
            }
            CelValue::Timestamp(ts) => Value::Timestamp(ts.fixed_offset()),
            CelValue::Uuid(id) => Value::Opaque(Arc::new(CelUuid(id))),
        }
    }

    pub(crate) fn from_cel_value(value: Value) -> Result<Self, CelError> {
        Ok(match value {
            Value::Map(map) => {
                let mut res = CelMap::new();
                for (k, v) in map.map.iter() {
                    res.inner
                        .insert(CelKey::try_from(k)?, CelValue::from_cel_value(v.clone())?);
                }
                CelValue::Map(Arc::new(res))
            }
            Value::List(values) => {
                let mut res = CelArray::new();
                for value in values.iter() {
                    res.push(CelValue::from_cel_value(value.clone())?);
                }
                CelValue::List(Arc::new(res))
            }
            Value::Int(i) => CelValue::Int(i),
            Value::UInt(u) => CelValue::UInt(u),
            Value::Float(f) => CelValue::Double(f),
            Value::String(s) => CelValue::String(s),
            Value::Bytes(b) => CelValue::Bytes(b),
            Value::Bool(b) => CelValue::Bool(b),
            Value::Duration(d) => CelValue::String(Arc::new(format!("{d:?}"))),
            Value::Timestamp(ts) => CelValue::Timestamp(ts.with_timezone(&Utc)),
            Value::Opaque(o) if o.runtime_type_name() == "cala.Decimal" => {
                let decimal = o.downcast_ref::<CelDecimal>().ok_or_else(|| {
                    CelError::Unexpected("Could not downcast decimal".to_string())
                })?;
                CelValue::Decimal(decimal.0)
            }
            Value::Opaque(o) if o.runtime_type_name() == "cala.Uuid" => {
                let id = o
                    .downcast_ref::<CelUuid>()
                    .ok_or_else(|| CelError::Unexpected("Could not downcast uuid".to_string()))?;
                CelValue::Uuid(id.0)
            }
            Value::Opaque(o) => {
                return Err(CelError::Unexpected(format!(
                    "Unsupported opaque value {}",
                    o.runtime_type_name()
                )))
            }
            Value::Null => CelValue::Null,
            Value::Function(_, _) => {
                return Err(CelError::Unexpected(
                    "Cannot convert function value".to_string(),
                ))
            }
        })
    }
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
                } else if let Some(f) = n.as_f64() {
                    CelValue::Double(f)
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

#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum CelKey {
    Int(i64),
    UInt(u64),
    Bool(bool),
    String(Arc<String>),
}

impl CelKey {
    fn into_cel_key(self) -> Key {
        match self {
            CelKey::Int(i) => Key::Int(i),
            CelKey::UInt(u) => Key::Uint(u),
            CelKey::Bool(b) => Key::Bool(b),
            CelKey::String(s) => Key::String(s),
        }
    }
}

impl TryFrom<&Key> for CelKey {
    type Error = CelError;

    fn try_from(key: &Key) -> Result<Self, Self::Error> {
        Ok(match key {
            Key::Int(i) => CelKey::Int(*i),
            Key::Uint(u) => CelKey::UInt(*u),
            Key::Bool(b) => CelKey::Bool(*b),
            Key::String(s) => CelKey::String(s.clone()),
        })
    }
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
            CelValue::Timestamp(_) => CelType::Timestamp,
        }
    }
}

impl From<DateTime<Utc>> for CelValue {
    fn from(d: DateTime<Utc>) -> Self {
        CelValue::Timestamp(d)
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

impl<'a> TryFrom<&'a CelValue> for &'a Decimal {
    type Error = CelError;

    fn try_from(v: &'a CelValue) -> Result<Self, Self::Error> {
        if let CelValue::Decimal(d) = v {
            Ok(d)
        } else {
            Err(CelError::BadType(CelType::Decimal, CelType::from(v)))
        }
    }
}

impl TryFrom<CelResult<'_>> for bool {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::Bool(b) = val {
            Ok(b)
        } else {
            Err(ResultCoercionError::BadCoreTypeCoercion(
                format!("{expr:?}"),
                CelType::from(&val),
                CelType::Bool,
            ))
        }
    }
}

impl TryFrom<CelResult<'_>> for NaiveDate {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        match val {
            CelValue::Date(d) => Ok(d),
            CelValue::Timestamp(ts) => Ok(ts.date_naive()),
            _ => Err(ResultCoercionError::BadCoreTypeCoercion(
                expr.to_string(),
                CelType::from(&val),
                CelType::Date,
            )),
        }
    }
}

impl TryFrom<CelResult<'_>> for DateTime<Utc> {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::Timestamp(d) = val {
            Ok(d)
        } else {
            Err(ResultCoercionError::BadCoreTypeCoercion(
                expr.to_string(),
                CelType::from(&val),
                CelType::Timestamp,
            ))
        }
    }
}

impl TryFrom<CelResult<'_>> for Uuid {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::Uuid(id) = val {
            Ok(id)
        } else {
            Err(ResultCoercionError::BadCoreTypeCoercion(
                expr.to_string(),
                CelType::from(&val),
                CelType::Uuid,
            ))
        }
    }
}

impl TryFrom<CelResult<'_>> for String {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        if let CelValue::String(s) = val {
            Ok(s.to_string())
        } else {
            Err(ResultCoercionError::BadCoreTypeCoercion(
                expr.to_string(),
                CelType::from(&val),
                CelType::String,
            ))
        }
    }
}

impl TryFrom<CelResult<'_>> for Decimal {
    type Error = ResultCoercionError;

    fn try_from(CelResult { expr, val }: CelResult) -> Result<Self, Self::Error> {
        match val {
            CelValue::Decimal(n) => Ok(n),
            _ => Err(ResultCoercionError::BadCoreTypeCoercion(
                expr.to_string(),
                CelType::from(&val),
                CelType::Decimal,
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
    type Error = ResultCoercionError;

    fn try_from(v: &CelKey) -> Result<Self, Self::Error> {
        if let CelKey::String(s) = v {
            Ok(s.to_string())
        } else {
            Err(ResultCoercionError::BadCoreTypeCoercion(
                format!("{v:?}"),
                CelType::from(v),
                CelType::String,
            ))
        }
    }
}

impl TryFrom<CelResult<'_>> for serde_json::Value {
    type Error = ResultCoercionError;

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
            CelValue::Timestamp(d) => Value::from(d.to_rfc3339()),
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
            CelValue::Decimal(d) => Value::from(d.to_string()),
            CelValue::Bytes(_) => unimplemented!(),
        })
    }
}
