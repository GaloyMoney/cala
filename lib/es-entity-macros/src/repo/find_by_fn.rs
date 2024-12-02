use darling::ToTokens;
use proc_macro2::{Span, TokenStream};
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct FindByFn<'a> {
    ignore_prefix: Option<&'a syn::LitStr>,
    entity: &'a syn::Ident,
    column: &'a Column,
    table_name: &'a str,
    error: &'a syn::Type,
    delete: DeleteOption,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> FindByFn<'a> {
    pub fn new(column: &'a Column, opts: &'a RepositoryOptions) -> Self {
        Self {
            ignore_prefix: opts.table_prefix(),
            column,
            entity: opts.entity(),
            table_name: opts.table_name(),
            error: opts.err(),
            delete: opts.delete,
            nested_fn_names: opts.all_nested().map(|f| f.find_nested_fn_name()).collect(),
        }
    }
}

impl<'a> ToTokens for FindByFn<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let entity = self.entity;
        let column_name = &self.column.name();
        let column_type = &self.column.ty();
        let error = self.error;
        let nested = self.nested_fn_names.iter().map(|f| {
            quote! {
                self.#f(&mut entities).await?;
            }
        });
        let maybe_lookup_nested = if self.nested_fn_names.is_empty() {
            quote! {}
        } else {
            quote! {
                let mut entities = vec![entity];
                #(#nested)*
                let entity = entities.pop().unwrap();
            }
        };
        let prefix_arg = self.ignore_prefix.map(|p| quote! { #p, });

        for delete in [DeleteOption::No, DeleteOption::Soft] {
            let fn_name = syn::Ident::new(
                &format!(
                    "find_by_{}{}",
                    column_name,
                    delete.include_deletion_fn_postfix()
                ),
                Span::call_site(),
            );
            let fn_via = syn::Ident::new(
                &format!(
                    "find_by_{}_via{}",
                    column_name,
                    delete.include_deletion_fn_postfix()
                ),
                Span::call_site(),
            );
            let fn_in_tx = syn::Ident::new(
                &format!(
                    "find_by_{}_in_tx{}",
                    column_name,
                    delete.include_deletion_fn_postfix()
                ),
                Span::call_site(),
            );

            let query = format!(
                r#"SELECT id FROM {} WHERE {} = $1{}"#,
                self.table_name,
                column_name,
                if delete == DeleteOption::No {
                    self.delete.not_deleted_condition()
                } else {
                    ""
                }
            );

            tokens.append_all(quote! {
                pub async fn #fn_name(
                    &self,
                    #column_name: impl std::borrow::Borrow<#column_type>
                ) -> Result<#entity, #error> {
                    self.#fn_via(self.pool(), #column_name).await
                }

                pub async fn #fn_in_tx(
                    &self,
                    db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                    #column_name: impl std::borrow::Borrow<#column_type>
                ) -> Result<#entity, #error> {
                    self.#fn_via(&mut **db, #column_name).await
                }

                async fn #fn_via(
                    &self,
                    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
                    #column_name: impl std::borrow::Borrow<#column_type>
                ) -> Result<#entity, #error> {
                    let #column_name = #column_name.borrow();
                    let entity = es_entity::es_query!(
                        #prefix_arg
                        executor,
                        #query,
                        #column_name as &#column_type,
                    )
                        .fetch_one()
                        .await?;
                    #maybe_lookup_nested
                    Ok(entity)
                }
            });

            if delete == self.delete {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::Ident;

    #[test]
    fn find_by_fn() {
        let column = Column::for_id(syn::parse_str("EntityId").unwrap());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        let persist_fn = FindByFn {
            ignore_prefix: None,
            column: &column,
            entity: &entity,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::No,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn find_by_id(
                &self,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                self.find_by_id_via(self.pool(), id).await
            }

            pub async fn find_by_id_in_tx(
                &self,
                db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                self.find_by_id_via(&mut **db, id).await
            }

            async fn find_by_id_via(
                &self,
                executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                let id = id.borrow();
                let entity = es_entity::es_query!(
                        executor,
                        "SELECT id FROM entities WHERE id = $1",
                        id as &EntityId,
                )
                    .fetch_one()
                    .await?;
                Ok(entity)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn find_by_fn_with_soft_delete() {
        let column = Column::for_id(syn::parse_str("EntityId").unwrap());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        let persist_fn = FindByFn {
            ignore_prefix: None,
            column: &column,
            entity: &entity,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::Soft,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn find_by_id(
                &self,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                self.find_by_id_via(self.pool(), id).await
            }

            pub async fn find_by_id_in_tx(
                &self,
                db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                self.find_by_id_via(&mut **db, id).await
            }

            async fn find_by_id_via(
                &self,
                executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                let id = id.borrow();
                let entity = es_entity::es_query!(
                        executor,
                        "SELECT id FROM entities WHERE id = $1 AND deleted = FALSE",
                        id as &EntityId,
                )
                    .fetch_one()
                    .await?;
                Ok(entity)
            }

            pub async fn find_by_id_include_deleted(
                &self,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                self.find_by_id_via_include_deleted(self.pool(), id).await
            }

            pub async fn find_by_id_in_tx_include_deleted(
                &self,
                db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                self.find_by_id_via_include_deleted(&mut **db, id).await
            }

            async fn find_by_id_via_include_deleted(
                &self,
                executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
                id: impl std::borrow::Borrow<EntityId>
            ) -> Result<Entity, es_entity::EsRepoError> {
                let id = id.borrow();
                let entity = es_entity::es_query!(
                        executor,
                        "SELECT id FROM entities WHERE id = $1",
                        id as &EntityId,
                )
                    .fetch_one()
                    .await?;
                Ok(entity)
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
