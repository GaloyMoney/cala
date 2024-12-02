use darling::ToTokens;
use proc_macro2::{Span, TokenStream};
use quote::{quote, TokenStreamExt};

use super::{list_by_fn::CursorStruct, options::*};

pub struct ListForFn<'a> {
    ignore_prefix: Option<&'a syn::LitStr>,
    pub for_column: &'a Column,
    pub by_column: &'a Column,
    entity: &'a syn::Ident,
    id: &'a syn::Ident,
    table_name: &'a str,
    error: &'a syn::Type,
    delete: DeleteOption,
    cursor_mod: syn::Ident,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> ListForFn<'a> {
    pub fn new(for_column: &'a Column, by_column: &'a Column, opts: &'a RepositoryOptions) -> Self {
        Self {
            ignore_prefix: opts.table_prefix(),
            for_column,
            by_column,
            id: opts.id(),
            entity: opts.entity(),
            table_name: opts.table_name(),
            error: opts.err(),
            delete: opts.delete,
            cursor_mod: opts.cursor_mod(),
            nested_fn_names: opts.all_nested().map(|f| f.find_nested_fn_name()).collect(),
        }
    }

    pub fn cursor(&'a self) -> CursorStruct<'a> {
        CursorStruct {
            column: self.by_column,
            id: self.id,
            entity: self.entity,
            cursor_mod: &self.cursor_mod,
        }
    }
}

impl ToTokens for ListForFn<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let prefix_arg = self.ignore_prefix.map(|p| quote! { #p, });
        let entity = self.entity;
        let cursor = self.cursor();
        let cursor_ident = cursor.ident();
        let cursor_mod = cursor.cursor_mod();
        let error = self.error;
        let nested = self.nested_fn_names.iter().map(|f| {
            quote! {
                self.#f(&mut entities).await?;
            }
        });
        let maybe_mut_entities = if self.nested_fn_names.is_empty() {
            quote! { (entities, has_next_page) }
        } else {
            quote! { (mut entities, has_next_page) }
        };
        let maybe_lookup_nested = if self.nested_fn_names.is_empty() {
            quote! {}
        } else {
            quote! {
                {
                    #(#nested)*
                }
            }
        };

        let by_column_name = self.by_column.name();

        let for_column_name = self.for_column.name();
        let filter_arg_name = syn::Ident::new(
            &format!("filter_{}", self.for_column.name()),
            Span::call_site(),
        );
        let for_column_type = self.for_column.ty();

        let destructure_tokens = self.cursor().destructure_tokens();
        let select_columns = cursor.select_columns(Some(for_column_name));
        let arg_tokens = cursor.query_arg_tokens();

        for delete in [DeleteOption::No, DeleteOption::Soft] {
            let fn_name = syn::Ident::new(
                &format!(
                    "list_for_{}_by_{}{}",
                    for_column_name,
                    by_column_name,
                    delete.include_deletion_fn_postfix()
                ),
                Span::call_site(),
            );

            let asc_query = format!(
                r#"SELECT {} FROM {} WHERE (({} = $1) AND ({})){} ORDER BY {} LIMIT $2"#,
                select_columns,
                self.table_name,
                for_column_name,
                cursor.condition(1, true),
                if delete == DeleteOption::No {
                    self.delete.not_deleted_condition()
                } else {
                    ""
                },
                cursor.order_by(true)
            );
            let desc_query = format!(
                r#"SELECT {} FROM {} WHERE (({} = $1) AND ({})){} ORDER BY {} LIMIT $2"#,
                select_columns,
                self.table_name,
                for_column_name,
                cursor.condition(1, false),
                if delete == DeleteOption::No {
                    self.delete.not_deleted_condition()
                } else {
                    ""
                },
                cursor.order_by(false)
            );

            tokens.append_all(quote! {
                pub async fn #fn_name(
                    &self,
                    #filter_arg_name: #for_column_type,
                    cursor: es_entity::PaginatedQueryArgs<#cursor_mod::#cursor_ident>,
                    direction: es_entity::ListDirection,
                ) -> Result<es_entity::PaginatedQueryRet<#entity, #cursor_mod::#cursor_ident>, #error> {
                    #destructure_tokens

                    let #maybe_mut_entities = match direction {
                        es_entity::ListDirection::Ascending => {
                            es_entity::es_query!(
                                #prefix_arg
                                self.pool(),
                                #asc_query,
                                #filter_arg_name as #for_column_type,
                                #arg_tokens
                            )
                                .fetch_n(first)
                                .await?
                        },
                        es_entity::ListDirection::Descending => {
                            es_entity::es_query!(
                                #prefix_arg
                                self.pool(),
                                #desc_query,
                                #filter_arg_name as #for_column_type,
                                #arg_tokens
                            )
                                .fetch_n(first)
                                .await?
                        }
                    };

                    #maybe_lookup_nested

                    let end_cursor = entities.last().map(#cursor_mod::#cursor_ident::from);

                    Ok(es_entity::PaginatedQueryRet {
                        entities,
                        has_next_page,
                        end_cursor,
                    })
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
    fn list_for_fn() {
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let id = syn::Ident::new("EntityId", proc_macro2::Span::call_site());
        let by_column = Column::for_id(syn::parse_str("EntityId").unwrap());
        let for_column = Column::new(
            syn::Ident::new("customer_id", proc_macro2::Span::call_site()),
            syn::parse_str("Uuid").unwrap(),
        );
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let persist_fn = ListForFn {
            ignore_prefix: None,
            entity: &entity,
            id: &id,
            for_column: &for_column,
            by_column: &by_column,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::No,
            cursor_mod,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn list_for_customer_id_by_id(
                &self,
                filter_customer_id: Uuid,
                cursor: es_entity::PaginatedQueryArgs<cursor_mod::EntitiesByIdCursor>,
                direction: es_entity::ListDirection,
            ) -> Result<es_entity::PaginatedQueryRet<Entity, cursor_mod::EntitiesByIdCursor>, es_entity::EsRepoError> {
                let es_entity::PaginatedQueryArgs { first, after } = cursor;
                let id = if let Some(after) = after {
                    Some(after.id)
                } else {
                    None
                };
                let (entities, has_next_page) = match direction {
                    es_entity::ListDirection::Ascending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT customer_id, id FROM entities WHERE ((customer_id = $1) AND (COALESCE(id > $3, true))) ORDER BY id ASC LIMIT $2",
                            filter_customer_id as Uuid,
                            (first + 1) as i64,
                            id as Option<EntityId>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT customer_id, id FROM entities WHERE ((customer_id = $1) AND (COALESCE(id < $3, true))) ORDER BY id DESC LIMIT $2",
                            filter_customer_id as Uuid,
                            (first + 1) as i64,
                            id as Option<EntityId>,
                        )
                            .fetch_n(first)
                            .await?
                    }
                };

                    let end_cursor = entities.last().map(cursor_mod::EntitiesByIdCursor::from);
                    Ok(es_entity::PaginatedQueryRet {
                        entities,
                        has_next_page,
                        end_cursor,
                    })
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn list_same_column() {
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let id = syn::Ident::new("EntityId", proc_macro2::Span::call_site());
        let column = Column::new(
            syn::Ident::new("email", proc_macro2::Span::call_site()),
            syn::parse_str("String").unwrap(),
        );
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let persist_fn = ListForFn {
            ignore_prefix: None,
            entity: &entity,
            id: &id,
            for_column: &column,
            by_column: &column,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::No,
            cursor_mod,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn list_for_email_by_email(
                &self,
                filter_email: String,
                cursor: es_entity::PaginatedQueryArgs<cursor_mod::EntitiesByEmailCursor>,
                direction: es_entity::ListDirection,
            ) -> Result<es_entity::PaginatedQueryRet<Entity, cursor_mod::EntitiesByEmailCursor>, es_entity::EsRepoError> {
                let es_entity::PaginatedQueryArgs { first, after } = cursor;
                let (id, email) = if let Some(after) = after {
                    (Some(after.id), Some(after.email))
                } else {
                    (None, None)
                };
                let (entities, has_next_page) = match direction {
                    es_entity::ListDirection::Ascending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT email, id FROM entities WHERE ((email = $1) AND (COALESCE((email, id) > ($4, $3), $3 IS NULL))) ORDER BY email ASC, id ASC LIMIT $2",
                            filter_email as String,
                            (first + 1) as i64,
                            id as Option<EntityId>,
                            email as Option<String>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT email, id FROM entities WHERE ((email = $1) AND (COALESCE((email, id) < ($4, $3), $3 IS NULL))) ORDER BY email DESC, id DESC LIMIT $2",
                            filter_email as String,
                            (first + 1) as i64,
                            id as Option<EntityId>,
                            email as Option<String>,
                        )
                            .fetch_n(first)
                            .await?
                    }
                };

                let end_cursor = entities.last().map(cursor_mod::EntitiesByEmailCursor::from);
                Ok(es_entity::PaginatedQueryRet {
                    entities,
                    has_next_page,
                    end_cursor,
                })
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
