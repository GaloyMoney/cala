use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct CreateAllFn<'a> {
    entity: &'a syn::Ident,
    table_name: &'a str,
    columns: &'a Columns,
    error: &'a syn::Type,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> From<&'a RepositoryOptions> for CreateAllFn<'a> {
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

impl<'a> ToTokens for CreateAllFn<'a> {
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
            .variable_assignments_for_create_all(syn::parse_quote! { new_entity });

        let table_name = self.table_name;

        let column_names = self.columns.insert_column_names();
        let builder_args = self.columns.create_query_builder_args();

        let insert_fragment = format!(
            "INSERT INTO {} ({}, created_at)",
            table_name,
            column_names.join(", "),
        );

        tokens.append_all(quote! {
            pub async fn create_all(
                &self,
                new_entities: Vec<<#entity as es_entity::EsEntity>::New>
            ) -> Result<Vec<#entity>, #error> {
                let mut op = self.begin_op().await?;
                let res = self.create_all_in_op(&mut op, new_entities).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn create_all_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                new_entities: Vec<<#entity as es_entity::EsEntity>::New>
            ) -> Result<Vec<#entity>, #error> {
                let mut query_builder: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                    #insert_fragment,
                );
                query_builder.push_values(new_entities.iter(), |mut builder, new_entity: &<#entity as es_entity::EsEntity>::New| {
                    #assignments
                    #(#builder_args)*
                    builder.push_bind(op.now());
                });
                let query = query_builder.build();
                query.execute(&mut **op.tx()).await?;


                let mut all_events: Vec<es_entity::EntityEvents<<#entity as es_entity::EsEntity>::Event>> = new_entities.into_iter().map(Self::convert_new).collect();
                let mut n_persisted = self.persist_events_batch(op, &mut all_events).await?;

                let mut res = Vec::new();
                for events in all_events.into_iter() {
                    let n_events = n_persisted.remove(events.id()).expect("n_events exists");
                    let #maybe_mut_entity = Self::hydrate_entity(events)?;

                    #(#nested)*

                    self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
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
    use syn::Ident;

    #[test]
    fn create_all_fn() {
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        use darling::FromMeta;
        let input: syn::Meta = syn::parse_quote!(columns(id = "EntityId", name = "String",));
        let columns = Columns::from_meta(&input).expect("Failed to parse Fields");

        let create_fn = CreateAllFn {
            table_name: "entities",
            entity: &entity,
            error: &error,
            columns: &columns,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        create_fn.to_tokens(&mut tokens);

        let mut tokens = TokenStream::new();
        create_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn create_all(
                &self,
                new_entities: Vec<<Entity as es_entity::EsEntity>::New>
            ) -> Result<Vec<Entity>, es_entity::EsRepoError> {
                let mut op = self.begin_op().await?;
                let res = self.create_all_in_op(&mut op, new_entities).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn create_all_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                new_entities: Vec<<Entity as es_entity::EsEntity>::New>
            ) -> Result<Vec<Entity>, es_entity::EsRepoError> {
                let mut query_builder: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                    "INSERT INTO entities (id, name, created_at)",
                );

                query_builder.push_values(new_entities.iter(), |mut builder, new_entity: &<Entity as es_entity::EsEntity>::New| {
                    let id: &EntityId = &new_entity.id;
                    let name: &String = &new_entity.name;

                    builder.push_bind(id);
                    builder.push_bind(name);
                    builder.push_bind(op.now());
                });

                let query = query_builder.build();
                query.execute(&mut **op.tx()).await?;

                let mut all_events: Vec<es_entity::EntityEvents<<#entity as es_entity::EsEntity>::Event>> = new_entities.into_iter().map(Self::convert_new).collect();
                let mut n_persisted = self.persist_events_batch(op, &mut all_events).await?;

                let mut res = Vec::new();
                for events in all_events.into_iter() {
                    let n_events = n_persisted.remove(events.id()).expect("n_events exists");
                    let entity = Self::hydrate_entity(events)?;

                    self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                    res.push(entity);
                }

                Ok(res)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
