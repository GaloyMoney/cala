#[macro_export]
macro_rules! idempotency_guard {
    ($events:expr, $( $pattern:pat $(if $guard:expr)? ),+ $(,)?) => {
        for event in $events {
            match event {
                $(
                    $pattern $(if $guard)? => return $crate::FromIdempotentIgnored::from_ignored(),
                )+
                _ => {}
            }
        }
    };
    ($events:expr, $( $pattern:pat $(if $guard:expr)? ),+,
     => $break_pattern:pat $(if $break_guard:expr)?) => {
        for event in $events {
            match event {
                $($pattern $(if $guard)? => return $crate::FromIdempotentIgnored::from_ignored(),)+
                $break_pattern $(if $break_guard)? => break,
                _ => {}
            }
        }
    };
}

#[macro_export]
macro_rules! es_query {
    ($prefix:literal, $db:expr, $query:expr) => ({
        $crate::expand_es_query!(
            ignore_prefix = $prefix,
            executor = $db,
            sql = $query
        )
    });
    ($prefix:literal, $db:expr, $query:expr, $($args:tt)*) => ({
        $crate::expand_es_query!(
            ignore_prefix = $prefix,
            executor = $db,
            sql = $query,
            args = [$($args)*]
        )
    });
    ($db:expr, $query:expr) => ({
        $crate::expand_es_query!(
            executor = $db,
            sql = $query
        )
    });
    ($db:expr, $query:expr, $($args:tt)*) => ({
        $crate::expand_es_query!(
            executor = $db,
            sql = $query,
            args = [$($args)*]
        )
    });
}

#[macro_export]
macro_rules! from_es_entity_error {
    ($name:ident) => {
        impl $name {
            pub fn was_not_found(&self) -> bool {
                matches!(self, $name::EsEntityError($crate::EsEntityError::NotFound))
            }
            pub fn was_concurrent_modification(&self) -> bool {
                matches!(
                    self,
                    $name::EsEntityError($crate::EsEntityError::ConcurrentModification)
                )
            }
        }
        impl From<$crate::EsEntityError> for $name {
            fn from(e: $crate::EsEntityError) -> Self {
                $name::EsEntityError(e)
            }
        }
    };
}

// Helper macro for common entity_id derives
#[allow(unused_macros)]
macro_rules! entity_id_derives {
    (json_schema) => {
        #[derive(
            $crate::prelude::sqlx::Type,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            $crate::prelude::serde::Deserialize,
            $crate::prelude::serde::Serialize,
            $crate::prelude::schemars::JsonSchema,
        )]
        #[serde(transparent)]
        #[sqlx(transparent)]
    };
    (no_json_schema) => {
        #[derive(
            $crate::prelude::sqlx::Type,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            $crate::prelude::serde::Deserialize,
            $crate::prelude::serde::Serialize,
        )]
        #[serde(transparent)]
        #[sqlx(transparent)]
    };
}

// Helper macro for common entity_id implementations
#[allow(unused_macros)]
macro_rules! entity_id_common_impls {
    ($name:ident) => {
        impl $name {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                $crate::prelude::uuid::Uuid::new_v4().into()
            }
        }

        impl From<$crate::prelude::uuid::Uuid> for $name {
            fn from(uuid: $crate::prelude::uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for $crate::prelude::uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl From<&$name> for $crate::prelude::uuid::Uuid {
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
            type Err = $crate::prelude::uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self($crate::prelude::uuid::Uuid::parse_str(s)?))
            }
        }
    };
}

// Helper macro for GraphQL-specific entity_id implementations
#[allow(unused_macros)]
macro_rules! entity_id_graphql_impls {
    ($name:ident) => {
        impl From<$crate::graphql::UUID> for $name {
            fn from(id: $crate::graphql::UUID) -> Self {
                $name($crate::prelude::uuid::Uuid::from(&id))
            }
        }

        impl From<&$crate::graphql::UUID> for $name {
            fn from(id: &$crate::graphql::UUID) -> Self {
                $name($crate::prelude::uuid::Uuid::from(id))
            }
        }
    };
}

// Helper macro for additional conversions
#[allow(unused_macros)]
macro_rules! entity_id_conversions {
    ($($from:ty => $to:ty),* $(,)?) => {
        $(
            impl From<$from> for $to {
                fn from(id: $from) -> Self {
                    <$to>::from($crate::prelude::uuid::Uuid::from(id))
                }
            }
            impl From<$to> for $from {
                fn from(id: $to) -> Self {
                    <$from>::from($crate::prelude::uuid::Uuid::from(id))
                }
            }
        )*
    };
}

#[cfg(all(feature = "graphql", feature = "json-schema"))]
#[macro_export]
macro_rules! entity_id {
    // Match identifiers without conversions
    ($($name:ident),+ $(,)?) => {
        $crate::entity_id! { $($name),+ ; }
    };
    ($($name:ident),+ $(,)? ; $($from:ty => $to:ty),* $(,)?) => {
        $(
            entity_id_derives!(json_schema)
            pub struct $name($crate::prelude::uuid::Uuid);
            entity_id_common_impls!($name);
            entity_id_graphql_impls!($name);
        )+
        entity_id_conversions!($($from => $to),*);
    };
}

#[cfg(all(feature = "graphql", not(feature = "json-schema")))]
#[macro_export]
macro_rules! entity_id {
    // Match identifiers without conversions
    ($($name:ident),+ $(,)?) => {
        $crate::entity_id! { $($name),+ ; }
    };
    ($($name:ident),+ $(,)? ; $($from:ty => $to:ty),* $(,)?) => {
        $(
            entity_id_derives!(no_json_schema)
            pub struct $name($crate::prelude::uuid::Uuid);
            entity_id_common_impls!($name);
            entity_id_graphql_impls!($name);
        )+
        entity_id_conversions!($($from => $to),*);
    };
}

#[cfg(all(feature = "json-schema", not(feature = "graphql")))]
#[macro_export]
macro_rules! entity_id {
    // Match identifiers without conversions
    ($($name:ident),+ $(,)?) => {
        $crate::entity_id! { $($name),+ ; }
    };
    ($($name:ident),+ $(,)? ; $($from:ty => $to:ty),* $(,)?) => {
        $(
            entity_id_derives!(json_schema)
            pub struct $name($crate::prelude::uuid::Uuid);
            entity_id_common_impls!($name);
        )+
        entity_id_conversions!($($from => $to),*);
    };
}

#[cfg(all(not(feature = "json-schema"), not(feature = "graphql")))]
#[macro_export]
macro_rules! entity_id {
    // Match identifiers without conversions
    ($($name:ident),+ $(,)?) => {
        $crate::entity_id! { $($name),+ ; }
    };
    ($($name:ident),+ $(,)? ; $($from:ty => $to:ty),* $(,)?) => {
        $(
            entity_id_derives!(no_json_schema)
            pub struct $name($crate::prelude::uuid::Uuid);
            entity_id_common_impls!($name);
        )+
        entity_id_conversions!($($from => $to),*);
    };
}
