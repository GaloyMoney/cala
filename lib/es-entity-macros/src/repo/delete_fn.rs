use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct DeleteFn<'a> {
    error: &'a syn::Type,
    entity: &'a syn::Ident,
    table_name: &'a str,
    columns: &'a Columns,
    delete_option: &'a DeleteOption,
}

impl<'a> DeleteFn<'a> {
    pub fn from(opts: &'a RepositoryOptions) -> Self {
        Self {
            entity: opts.entity(),
            error: opts.err(),
            columns: &opts.columns,
            table_name: opts.table_name(),
            delete_option: &opts.delete,
        }
    }
}

impl<'a> ToTokens for DeleteFn<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if matches!(self.delete_option, DeleteOption::No) {
            return;
        }

        let entity = self.entity;
        let error = self.error;

        let assignments = self
            .columns
            .variable_assignments_for_update(syn::parse_quote! { entity });
        let column_updates = self.columns.sql_updates();
        let query = format!(
            "UPDATE {} SET {}{}deleted = TRUE WHERE id = $1",
            self.table_name,
            column_updates,
            if column_updates.is_empty() { "" } else { ", " }
        );
        let args = self.columns.update_query_args();

        tokens.append_all(quote! {
            pub async fn delete_in_op(&self,
                op: &mut es_entity::DbOp<'_>,
                mut entity: #entity
            ) -> Result<(), #error> {
                #assignments

                sqlx::query!(
                    #query,
                    #(#args),*
                )
                    .execute(&mut **op.tx())
                    .await?;

                let new_events = {
                    let events = Self::extract_events(&mut entity);
                    events.any_new()
                };

                if new_events {
                    let n_events = {
                        let events = Self::extract_events(&mut entity);
                        self.persist_events(op, events).await?
                    };

                    self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                }

                Ok(())
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
    fn delete_fn() {
        let id = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let mut columns = Columns::default();
        columns.set_id_column(&id);

        let delete_fn = DeleteFn {
            entity: &entity,
            error: &error,
            table_name: "entities",
            columns: &columns,
            delete_option: &DeleteOption::Soft,
        };

        let mut tokens = TokenStream::new();
        delete_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn delete_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                mut entity: Entity
            ) -> Result<(), es_entity::EsRepoError> {
                let id = &entity.id;

                sqlx::query!(
                    "UPDATE entities SET deleted = TRUE WHERE id = $1",
                    id as &EntityId
                )
                    .execute(&mut **op.tx())
                    .await?;

                let new_events = {
                    let events = Self::extract_events(&mut entity);
                    events.any_new()
                };

                if new_events {
                    let n_events = {
                        let events = Self::extract_events(&mut entity);
                        self.persist_events(op, events).await?
                    };

                    self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                }

                Ok(())
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn delete_fn_with_update_columns() {
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

        let delete_fn = DeleteFn {
            entity: &entity,
            error: &error,
            table_name: "entities",
            columns: &columns,
            delete_option: &DeleteOption::Soft,
        };

        let mut tokens = TokenStream::new();
        delete_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn delete_in_op(
                &self,
                op: &mut es_entity::DbOp<'_>,
                mut entity: Entity
            ) -> Result<(), es_entity::EsRepoError> {
                let id = &entity.id;
                let name = &entity.name;

                sqlx::query!(
                    "UPDATE entities SET name = $2, deleted = TRUE WHERE id = $1",
                    id as &EntityId,
                    name as &String
                )
                    .execute(&mut **op.tx())
                    .await?;

                let new_events = {
                    let events = Self::extract_events(&mut entity);
                    events.any_new()
                };

                if new_events {
                    let n_events = {
                        let events = Self::extract_events(&mut entity);
                        self.persist_events(op, events).await?
                    };

                    self.execute_post_persist_hook(op, &entity, entity.events().last_persisted(n_events)).await?;
                }

                Ok(())
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
