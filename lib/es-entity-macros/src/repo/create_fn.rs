use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct CreateFn<'a> {
    entity: &'a syn::Ident,
    table_name: &'a str,
    columns: &'a Columns,
    error: &'a syn::Type,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> From<&'a RepositoryOptions> for CreateFn<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        Self {
            table_name: opts.table_name(),
            entity: opts.entity(),
            error: opts.err(),
            nested_fn_names: opts
                .all_nested()
                .map(|f| f.create_nested_fn_name())
                .collect(),
            columns: &opts.columns,
        }
    }
}

impl<'a> ToTokens for CreateFn<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let entity = self.entity;
        let error = self.error;

        let nested = self.nested_fn_names.iter().map(|f| {
            quote! {
                self.#f(op, &mut entity).await?;
            }
        });
        let maybe_mut_entity = if self.nested_fn_names.is_empty() {
            quote! { entity }
        } else {
            quote! { mut entity }
        };
        let assignments = self
            .columns
            .variable_assignments_for_create(syn::parse_quote! { new_entity });

        let table_name = self.table_name;

        let column_names = self.columns.insert_column_names();
        let placeholders = self.columns.insert_placeholders();
        let args = self.columns.create_query_args();

        let query = format!(
            "INSERT INTO {} ({}, created_at) VALUES ({}, ${})",
            table_name,
            column_names.join(", "),
            placeholders,
            column_names.len() + 1,
        );

        tokens.append_all(quote! {
            #[inline(always)]
            fn convert_new<T, E>(item: T) -> es_entity::EntityEvents<E>
            where
                T: es_entity::IntoEvents<E>,
                E: es_entity::EsEvent,
            {
                item.into_events()
            }

            #[inline(always)]
            fn hydrate_entity<T, E>(events: es_entity::EntityEvents<E>) -> Result<T, #error>
            where
                T: es_entity::TryFromEvents<E>,
                #error: From<es_entity::EsEntityError>,
                E: es_entity::EsEvent,
            {
                Ok(T::try_from_events(events)?)
            }

            pub async fn create(
                &self,
                new_entity: <#entity as es_entity::EsEntity>::New
            ) -> Result<#entity, #error> {
                let mut op = self.begin_op().await?;
                let res = self.create_in_op(&mut op, new_entity).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn create_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                new_entity: <#entity as es_entity::EsEntity>::New
            ) -> Result<#entity, #error> {
                #assignments

                 sqlx::query!(
                     #query,
                     #(#args)*
                     op.now()
                )
                .execute(&mut **op.tx())
                .await?;

                let mut events = Self::convert_new(new_entity);
                let n_events = self.persist_events(op, &mut events).await?;
                let #maybe_mut_entity = Self::hydrate_entity(events)?;

                #(#nested)*

                self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                Ok(entity)
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::Ident;

    #[test]
    fn create_fn() {
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let id = Ident::new("EntityId", Span::call_site());
        let mut columns = Columns::default();
        columns.set_id_column(&id);

        let create_fn = CreateFn {
            table_name: "entities",
            entity: &entity,
            error: &error,
            columns: &columns,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        create_fn.to_tokens(&mut tokens);

        let expected = quote! {
            #[inline(always)]
            fn convert_new<T, E>(item: T) -> es_entity::EntityEvents<E>
            where
                T: es_entity::IntoEvents<E>,
                E: es_entity::EsEvent,
            {
                item.into_events()
            }

            #[inline(always)]
            fn hydrate_entity<T, E>(events: es_entity::EntityEvents<E>) -> Result<T, es_entity::EsRepoError>
            where
                T: es_entity::TryFromEvents<E>,
                es_entity::EsRepoError: From<es_entity::EsEntityError>,
                E: es_entity::EsEvent,
            {
                Ok(T::try_from_events(events)?)
            }

            pub async fn create(
                &self,
                new_entity: <Entity as es_entity::EsEntity>::New
            ) -> Result<Entity, es_entity::EsRepoError> {
                let mut op = self.begin_op().await?;
                let res = self.create_in_op(&mut op, new_entity).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn create_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                new_entity: <Entity as es_entity::EsEntity>::New
            ) -> Result<Entity, es_entity::EsRepoError> {
                let id = &new_entity.id;

                sqlx::query!("INSERT INTO entities (id, created_at) VALUES ($1, $2)",
                    id as &EntityId,
                    op.now()
                )
                .execute(&mut **op.tx())
                .await?;

                let mut events = Self::convert_new(new_entity);
                let n_events = self.persist_events(op, &mut events).await?;
                let entity = Self::hydrate_entity(events)?;

                self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                Ok(entity)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn create_fn_with_columns() {
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        use darling::FromMeta;
        let input: syn::Meta = syn::parse_quote!(columns(
            id = "EntityId",
            name(ty = "String", create(accessor = "name()"))
        ));
        let columns = Columns::from_meta(&input).expect("Failed to parse Fields");

        let create_fn = CreateFn {
            table_name: "entities",
            entity: &entity,
            error: &error,
            columns: &columns,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        create_fn.to_tokens(&mut tokens);

        let expected = quote! {
            #[inline(always)]
            fn convert_new<T, E>(item: T) -> es_entity::EntityEvents<E>
            where
                T: es_entity::IntoEvents<E>,
                E: es_entity::EsEvent,
            {
                item.into_events()
            }

            #[inline(always)]
            fn hydrate_entity<T, E>(events: es_entity::EntityEvents<E>) -> Result<T, es_entity::EsRepoError>
            where
                T: es_entity::TryFromEvents<E>,
                es_entity::EsRepoError: From<es_entity::EsEntityError>,
                E: es_entity::EsEvent,
            {
                Ok(T::try_from_events(events)?)
            }

            pub async fn create(
                &self,
                new_entity: <Entity as es_entity::EsEntity>::New
            ) -> Result<Entity, es_entity::EsRepoError> {
                let mut op = self.begin_op().await?;
                let res = self.create_in_op(&mut op, new_entity).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn create_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                new_entity: <Entity as es_entity::EsEntity>::New
            ) -> Result<Entity, es_entity::EsRepoError> {
                let id = &new_entity.id;
                let name = &new_entity.name();

                sqlx::query!("INSERT INTO entities (id, name, created_at) VALUES ($1, $2, $3)",
                    id as &EntityId,
                    name as &String,
                    op.now()
                )
                .execute(&mut **op.tx())
                .await?;

                let mut events = Self::convert_new(new_entity);
                let n_events = self.persist_events(op, &mut events).await?;
                let entity = Self::hydrate_entity(events)?;

                self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                Ok(entity)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
