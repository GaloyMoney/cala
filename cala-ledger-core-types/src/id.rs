#[macro_export]
macro_rules! entity_id {
    ($name:ident) => {
        #[derive(
            sqlx::Type,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Deserialize,
            serde::Serialize,
        )]
        #[serde(transparent)]
        #[sqlx(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                uuid::Uuid::new_v4().into()
            }

            pub fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }
        }

        impl From<uuid::Uuid> for $name {
            fn from(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl From<&$name> for uuid::Uuid {
            fn from(id: &$name) -> Self {
                id.0
            }
        }

        impl From<$name> for cel_interpreter::CelValue {
            fn from(id: $name) -> Self {
                cel_interpreter::CelValue::Uuid(id.0)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(uuid::Uuid::parse_str(s)?))
            }
        }

        #[cfg(feature = "graphql")]
        impl From<$crate::graphql::UUID> for $name {
            fn from(id: $crate::graphql::UUID) -> Self {
                $name(uuid::Uuid::from(&id))
            }
        }

        #[cfg(feature = "graphql")]
        impl From<&$crate::graphql::UUID> for $name {
            fn from(id: &$crate::graphql::UUID) -> Self {
                $name(uuid::Uuid::from(id))
            }
        }
    };
}
