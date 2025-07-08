use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct UpdateAllFn<'a> {
    entity: &'a syn::Ident,
    table_name: &'a str,
    columns: &'a Columns,
    error: &'a syn::Type,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> From<&'a RepositoryOptions> for UpdateAllFn<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        Self {
            table_name: opts.table_name(),
            entity: opts.entity(),
            error: opts.err(),
            nested_fn_names: opts
                .all_nested()
                .map(|f| f.update_nested_fn_name())
                .collect(),
            columns: &opts.columns,
        }
    }
}

impl ToTokens for UpdateAllFn<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let entity = self.entity;
        let error = self.error;
        let table_name = self.table_name;

        let column_names = self.columns.update_column_names().join(", ");
        let sql_updates = self.columns.sql_update_batched();

        let update_fragment = format!("UPDATE {} SET {} FROM (VALUES ", table_name, sql_updates);

        let assignments = self
            .columns
            .variable_assignments_for_update_all(syn::parse_quote! { entity });
        let builder_args = self.columns.update_query_builder_args();

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

        tokens.append_all(quote! {
            pub async fn update_all(
                &self,
                entities: Vec<#entity>,
            ) -> Result<Vec<#entity>, #error> {
                let mut op = self.begin_op().await?;
                let res = self.update_all_in_op(&mut op, entities).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn update_all_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                entities: Vec<#entity>,
            ) -> Result<Vec<#entity>, #error> {
                let entities_to_update: Vec<#entity> = entities
                    .into_iter()
                    .filter(|entity| entity.events().any_new())
                    .collect();

                if entities_to_update.is_empty() {
                    return Ok(entities_to_update);
                }
                    let mut query_builder: sqlx::QueryBuilder<sqlx::Postgres> =
                        sqlx::QueryBuilder::new(#update_fragment);

                    query_builder.push_values(entities_to_update.iter(), |mut builder, entity| {
                        #assignments
                        #(#builder_args)*
                    });

                    query_builder.push(&format!(
                        ") AS v({}) WHERE {}.id = v.id",
                        #column_names,
                        #table_name
                    ));

                    query_builder.build().execute(&mut **op.tx()).await?;

                let mut all_events: Vec<_> = entities_to_update
                    .into_iter()
                    .map(|entity| entity.into_events())
                    .collect();

                let mut n_persisted = self.persist_events_batch(op, &mut all_events).await?;

                let mut res = Vec::with_capacity(all_events.len());
                for events in all_events {
                    let n_events = n_persisted
                        .remove(events.id())
                        .expect("n_events exists");
                    let mut #maybe_mut_entity = Self::hydrate_entity(events)?;

                    #(#nested)*

                    self.execute_post_persist_hook(
                        op,
                        &entity,
                        entity.events().last_persisted(n_events)
                    ).await?;

                    res.push(entity);
                }

                Ok(res)
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::{Ident, Type};

    #[test]
    fn update_all_fn() {
        let entity = Ident::new("Entity", Span::call_site());
        let error: Type = syn::parse_str("es_entity::EsRepoError").unwrap();

        use darling::FromMeta;
        let input: syn::Meta = syn::parse_quote! {
            columns(
                id = "EntityId",
                name = "String",
                sequence(ty = "i32", update(persist = false)),
            )
        };
        let columns = Columns::from_meta(&input).expect("Failed to parse Columns");

        let update_fn = UpdateAllFn {
            table_name: "entities",
            entity: &entity,
            error: &error,
            columns: &columns,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        update_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn update_all(
                &self,
                entities: Vec<Entity>,
            ) -> Result<Vec<Entity>, es_entity::EsRepoError> {
                let mut op = self.begin_op().await?;
                let res = self.update_all_in_op(&mut op, entities).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn update_all_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                entities: Vec<Entity>,
            ) -> Result<Vec<Entity>, es_entity::EsRepoError> {
                let entities_to_update: Vec<Entity> = entities
                    .into_iter()
                    .filter(|entity| entity.events().any_new())
                    .collect();

                if entities_to_update.is_empty() {
                    return Ok(entities_to_update);
                }

                let mut query_builder: sqlx::QueryBuilder<sqlx::Postgres> =
                    sqlx::QueryBuilder::new("UPDATE entities SET name = v.name FROM (VALUES ");

                query_builder.push_values(entities_to_update.iter(), |mut builder, entity| {
                    let id: &EntityId = &entity.id;
                    let name: &String = &entity.name;

                    builder.push_bind(id);
                    builder.push_bind(name);
                });

                query_builder.push(&format!(
                    ") AS v({}) WHERE {}.id = v.id",
                    "id, name",
                    "entities"
                ));

                query_builder.build().execute(&mut **op.tx()).await?;

                let mut all_events: Vec<_> = entities_to_update
                    .into_iter()
                    .map(|entity| entity.into_events())
                    .collect();

                let mut n_persisted = self.persist_events_batch(op, &mut all_events).await?;

                let mut res = Vec::with_capacity(all_events.len());
                for events in all_events {
                    let n_events = n_persisted
                        .remove(events.id())
                        .expect("n_events exists");

                    let mut entity = Self::hydrate_entity(events)?;

                    self.execute_post_persist_hook(
                        op,
                        &entity,
                        entity.events().last_persisted(n_events)
                    ).await?;

                    res.push(entity);
                }

                Ok(res)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
