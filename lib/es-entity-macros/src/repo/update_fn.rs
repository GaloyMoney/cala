use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct UpdateFn<'a> {
    entity: &'a syn::Ident,
    table_name: &'a str,
    columns: &'a Columns,
    error: &'a syn::Type,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> From<&'a RepositoryOptions> for UpdateFn<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        Self {
            entity: opts.entity(),
            error: opts.err(),
            columns: &opts.columns,
            table_name: opts.table_name(),
            nested_fn_names: opts
                .all_nested()
                .map(|f| f.update_nested_fn_name())
                .collect(),
        }
    }
}

impl<'a> ToTokens for UpdateFn<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let entity = self.entity;
        let error = self.error;

        let nested = self.nested_fn_names.iter().map(|f| {
            quote! {
                self.#f(op, entity).await?;
            }
        });

        let update_tokens = if self.columns.updates_needed() {
            let assignments = self
                .columns
                .variable_assignments_for_update(syn::parse_quote! { entity });
            let column_updates = self.columns.sql_updates();
            let query = format!(
                "UPDATE {} SET {} WHERE id = $1",
                self.table_name, column_updates,
            );
            let args = self.columns.update_query_args();
            Some(quote! {
            #assignments
            sqlx::query!(
                #query,
                #(#args),*
            )
                .execute(&mut **op.tx())
                .await?;
            })
        } else {
            None
        };

        tokens.append_all(quote! {
            #[inline(always)]
            fn extract_events<T, E>(entity: &mut T) -> &mut es_entity::EntityEvents<E>
            where
                T: es_entity::EsEntity<Event = E>,
                E: es_entity::EsEvent,
            {
                entity.events_mut()
            }

            pub async fn update(
                &self,
                entity: &mut #entity
            ) -> Result<bool, #error> {
                let mut op = self.begin_op().await?;
                let res = self.update_in_op(&mut op, entity).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn update_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                entity: &mut #entity
            ) -> Result<bool, #error> {
                #(#nested)*

                if !Self::extract_events(entity).any_new() {
                    return Ok(false);
                }

                #update_tokens
                let n_events = {
                    let events = Self::extract_events(entity);
                    self.persist_events(op, events).await?
                };

                self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;

                Ok(true)
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
    fn update_fn() {
        let id = syn::parse_str("EntityId").unwrap();
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        let columns = Columns::new(
            &id,
            [Column::new(
                Ident::new("name", Span::call_site()),
                syn::parse_str("String").unwrap(),
            )],
        );

        let update_fn = UpdateFn {
            entity: &entity,
            table_name: "entities",
            error: &error,
            columns: &columns,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        update_fn.to_tokens(&mut tokens);

        let expected = quote! {
            #[inline(always)]
            fn extract_events<T, E>(entity: &mut T) -> &mut es_entity::EntityEvents<E>
            where
                T: es_entity::EsEntity<Event = E>,
                E: es_entity::EsEvent,
            {
                entity.events_mut()
            }

            pub async fn update(
                &self,
                entity: &mut Entity
            ) -> Result<bool, es_entity::EsRepoError> {
                let mut op = self.begin_op().await?;
                let res = self.update_in_op(&mut op, entity).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn update_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                entity: &mut Entity
            ) -> Result<bool, es_entity::EsRepoError> {
                if !Self::extract_events(entity).any_new() {
                    return Ok(false);
                }

                let id = &entity.id;
                let name = &entity.name;
                sqlx::query!(
                    "UPDATE entities SET name = $2 WHERE id = $1",
                    id as &EntityId,
                    name as &String
                )
                    .execute(&mut **op.tx())
                    .await?;

                let n_events = {
                    let events = Self::extract_events(entity);
                    self.persist_events(op, events).await?
                };

                self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;

                Ok(true)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn update_fn_no_columns() {
        let id = syn::parse_str("EntityId").unwrap();
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        let mut columns = Columns::default();
        columns.set_id_column(&id);

        let update_fn = UpdateFn {
            entity: &entity,
            table_name: "entities",
            error: &error,
            columns: &columns,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        update_fn.to_tokens(&mut tokens);

        let expected = quote! {
            #[inline(always)]
            fn extract_events<T, E>(entity: &mut T) -> &mut es_entity::EntityEvents<E>
            where
                T: es_entity::EsEntity<Event = E>,
                E: es_entity::EsEvent,
            {
                entity.events_mut()
            }

            pub async fn update(
                &self,
                entity: &mut Entity
            ) -> Result<bool, es_entity::EsRepoError> {
                let mut op = self.begin_op().await?;
                let res = self.update_in_op(&mut op, entity).await?;
                op.commit().await?;
                Ok(res)
            }

            pub async fn update_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                entity: &mut Entity
            ) -> Result<bool, es_entity::EsRepoError> {
                if !Self::extract_events(entity).any_new() {
                    return Ok(false);
                }

                let n_events = {
                    let events = Self::extract_events(entity);
                    self.persist_events(op, events).await?
                };

                self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;

                Ok(true)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
