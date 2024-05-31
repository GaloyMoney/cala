#[macro_export]
macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name($crate::uuid::Uuid);

        impl $name {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self($crate::uuid::Uuid::new_v4())
            }
        }

        impl From<$crate::uuid::Uuid> for $name {
            fn from(uuid: $crate::uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for $crate::uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl From<&$name> for $crate::uuid::Uuid {
            fn from(id: &$name) -> Self {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self($crate::uuid::Uuid::parse_str(s)?))
            }
        }

        const _: () = {
            impl<'de> $crate::serde::Deserialize<'de> for $name {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: $crate::serde::Deserializer<'de>,
                {
                    let uuid = $crate::uuid::Uuid::deserialize(deserializer)?;
                    Ok(Self(uuid))
                }
            }

            impl $crate::serde::Serialize for $name {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: $crate::serde::Serializer,
                {
                    self.0.serialize(serializer)
                }
            }
        };
        const _: () = {
            use $crate::sqlx::decode::Decode;
            use $crate::sqlx::encode::{Encode, IsNull};
            use $crate::sqlx::error::BoxDynError;
            use $crate::sqlx::postgres::{
                PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef, Postgres,
            };
            use $crate::sqlx::types::Type;

            impl Type<Postgres> for $name {
                fn type_info() -> PgTypeInfo {
                    PgTypeInfo::with_name("UUID")
                }
            }

            impl PgHasArrayType for $name {
                fn array_type_info() -> PgTypeInfo {
                    PgTypeInfo::with_name("UUID[]")
                }
            }

            impl Encode<'_, Postgres> for $name {
                fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> IsNull {
                    buf.extend_from_slice(self.0.as_bytes());

                    IsNull::No
                }
            }

            impl Decode<'_, Postgres> for $name {
                fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
                    match value.format() {
                        PgValueFormat::Binary => {
                            Ok(Self($crate::uuid::Uuid::from_slice(value.as_bytes()?)?))
                        }
                        PgValueFormat::Text => value.as_str()?.parse(),
                    }
                    .map_err(Into::into)
                }
            }
        };
    };
}
