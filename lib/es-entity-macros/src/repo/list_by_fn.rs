use convert_case::{Case, Casing};
use darling::ToTokens;
use proc_macro2::{Span, TokenStream};
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct CursorStruct<'a> {
    pub id: &'a syn::Ident,
    pub entity: &'a syn::Ident,
    pub column: &'a Column,
    pub cursor_mod: &'a syn::Ident,
}

impl CursorStruct<'_> {
    fn name(&self) -> String {
        let entity_name = pluralizer::pluralize(&format!("{}", self.entity), 2, false);
        format!("{}_by_{}_cursor", entity_name, self.column.name()).to_case(Case::UpperCamel)
    }

    pub fn ident(&self) -> syn::Ident {
        syn::Ident::new(&self.name(), Span::call_site())
    }

    pub fn cursor_mod(&self) -> &syn::Ident {
        self.cursor_mod
    }

    pub fn select_columns(&self, for_column: Option<&syn::Ident>) -> String {
        let mut for_column_str = String::new();
        if let Some(for_column) = for_column {
            if self.column.name() != for_column {
                for_column_str = format!("{}, ", for_column);
            }
        }
        if self.column.is_id() {
            format!("{}id", for_column_str)
        } else {
            format!("{}{}, id", for_column_str, self.column.name())
        }
    }

    pub fn order_by(&self, ascending: bool) -> String {
        let dir = if ascending { "ASC" } else { "DESC" };
        let nulls = if ascending { "FIRST" } else { "LAST" };
        if self.column.is_id() {
            format!("id {dir}")
        } else if self.column.is_optional() {
            format!("{0} {dir} NULLS {nulls}, id {dir}", self.column.name())
        } else {
            format!("{} {dir}, id {dir}", self.column.name())
        }
    }

    pub fn condition(&self, offset: u32, ascending: bool) -> String {
        let comp = if ascending { ">" } else { "<" };
        let id_offset = offset + 2;
        let column_offset = offset + 3;

        if self.column.is_id() {
            format!("COALESCE(id {comp} ${id_offset}, true)")
        } else if self.column.is_optional() {
            format!(
                "({0} IS NOT DISTINCT FROM ${column_offset}) AND COALESCE(id {comp} ${id_offset}, true) OR COALESCE({0} {comp} ${column_offset}, {0} IS NOT NULL)",
                self.column.name(),
            )
        } else {
            format!(
                "COALESCE(({0}, id) {comp} (${column_offset}, ${id_offset}), ${id_offset} IS NULL)",
                self.column.name(),
            )
        }
    }

    pub fn query_arg_tokens(&self) -> TokenStream {
        let id = self.id;

        if self.column.is_id() {
            quote! {
                (first + 1) as i64,
                id as Option<#id>,
            }
        } else if self.column.is_optional() {
            let column_name = self.column.name();
            let column_type = self.column.ty();
            quote! {
                (first + 1) as i64,
                id as Option<#id>,
                #column_name as #column_type,
            }
        } else {
            let column_name = self.column.name();
            let column_type = self.column.ty();
            quote! {
                (first + 1) as i64,
                id as Option<#id>,
                #column_name as Option<#column_type>,
            }
        }
    }

    pub fn destructure_tokens(&self) -> TokenStream {
        let column_name = self.column.name();

        let mut after_args = quote! {
            (id, #column_name)
        };
        let mut after_destruction = quote! {
            (Some(after.id), Some(after.#column_name))
        };
        let mut after_default = quote! {
            (None, None)
        };

        if self.column.is_id() {
            after_args = quote! {
                id
            };
            after_destruction = quote! {
                Some(after.id)
            };
            after_default = quote! {
                None
            };
        } else if self.column.is_optional() {
            after_destruction = quote! {
                (Some(after.id), after.#column_name)
            };
        }

        quote! {
            let es_entity::PaginatedQueryArgs { first, after } = cursor;
            let #after_args = if let Some(after) = after {
                #after_destruction
            } else {
                #after_default
            };
        }
    }

    #[cfg(feature = "graphql")]
    pub fn gql_cursor(&self) -> TokenStream {
        let ident = self.ident();
        quote! {
            impl es_entity::graphql::async_graphql::connection::CursorType for #ident {
                type Error = String;

                fn encode_cursor(&self) -> String {
                    use es_entity::graphql::base64::{engine::general_purpose, Engine as _};
                    let json = es_entity::prelude::serde_json::to_string(&self).expect("could not serialize token");
                    general_purpose::STANDARD_NO_PAD.encode(json.as_bytes())
                }

                fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
                    use es_entity::graphql::base64::{engine::general_purpose, Engine as _};
                    let bytes = general_purpose::STANDARD_NO_PAD
                        .decode(s.as_bytes())
                        .map_err(|e| e.to_string())?;
                    let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                    es_entity::prelude::serde_json::from_str(&json).map_err(|e| e.to_string())
                }
            }
        }
    }
}

impl ToTokens for CursorStruct<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let entity = self.entity;
        let accessor = &self.column.accessor();
        let ident = self.ident();
        let id = &self.id;

        let (field, from_impl) = if self.column.is_id() {
            (quote! {}, quote! {})
        } else {
            let column_name = self.column.name();
            let column_type = self.column.ty();
            (
                quote! {
                    pub #column_name: #column_type,
                },
                quote! {
                    #column_name: entity.#accessor.clone(),
                },
            )
        };

        tokens.append_all(quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub struct #ident {
                pub id: #id,
                #field
            }

            impl From<&#entity> for #ident {
                fn from(entity: &#entity) -> Self {
                    Self {
                        id: entity.id.clone(),
                        #from_impl
                    }
                }
            }
        });
    }
}

pub struct ListByFn<'a> {
    ignore_prefix: Option<&'a syn::LitStr>,
    id: &'a syn::Ident,
    entity: &'a syn::Ident,
    column: &'a Column,
    table_name: &'a str,
    error: &'a syn::Type,
    delete: DeleteOption,
    cursor_mod: syn::Ident,
    nested_fn_names: Vec<syn::Ident>,
}

impl<'a> ListByFn<'a> {
    pub fn new(column: &'a Column, opts: &'a RepositoryOptions) -> Self {
        Self {
            ignore_prefix: opts.table_prefix(),
            column,
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
            column: self.column,
            id: self.id,
            entity: self.entity,
            cursor_mod: &self.cursor_mod,
        }
    }
}

impl ToTokens for ListByFn<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let prefix_arg = self.ignore_prefix.map(|p| quote! { #p, });
        let entity = self.entity;
        let column_name = self.column.name();
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

        let destructure_tokens = self.cursor().destructure_tokens();
        let select_columns = cursor.select_columns(None);
        let arg_tokens = cursor.query_arg_tokens();

        for delete in [DeleteOption::No, DeleteOption::Soft] {
            let fn_name = syn::Ident::new(
                &format!(
                    "list_by_{}{}",
                    column_name,
                    delete.include_deletion_fn_postfix()
                ),
                Span::call_site(),
            );
            let asc_query = format!(
                r#"SELECT {} FROM {} WHERE ({}){} ORDER BY {} LIMIT $1"#,
                select_columns,
                self.table_name,
                cursor.condition(0, true),
                if delete == DeleteOption::No {
                    self.delete.not_deleted_condition()
                } else {
                    ""
                },
                cursor.order_by(true),
            );
            let desc_query = format!(
                r#"SELECT {} FROM {} WHERE ({}){} ORDER BY {} LIMIT $1"#,
                select_columns,
                self.table_name,
                cursor.condition(0, false),
                if delete == DeleteOption::No {
                    self.delete.not_deleted_condition()
                } else {
                    ""
                },
                cursor.order_by(false),
            );

            tokens.append_all(quote! {
                pub async fn #fn_name(
                    &self,
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
                                #arg_tokens
                            )
                                .fetch_n(first)
                                .await?
                        },
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
    fn cursor_struct_by_id() {
        let id_type = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let by_column = Column::for_id(syn::parse_str("EntityId").unwrap());
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let cursor = CursorStruct {
            column: &by_column,
            id: &id_type,
            entity: &entity,
            cursor_mod: &cursor_mod,
        };

        let mut tokens = TokenStream::new();
        cursor.to_tokens(&mut tokens);

        let expected = quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub struct EntitiesByIdCursor {
                pub id: EntityId,
            }

            impl From<&Entity> for EntitiesByIdCursor {
                fn from(entity: &Entity) -> Self {
                    Self {
                        id: entity.id.clone(),
                    }
                }
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn cursor_struct_by_created_at() {
        let id_type = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let by_column = Column::for_created_at();
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let cursor = CursorStruct {
            column: &by_column,
            id: &id_type,
            entity: &entity,
            cursor_mod: &cursor_mod,
        };

        let mut tokens = TokenStream::new();
        cursor.to_tokens(&mut tokens);

        let expected = quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub struct EntitiesByCreatedAtCursor {
                pub id: EntityId,
                pub created_at: es_entity::prelude::chrono::DateTime<es_entity::prelude::chrono::Utc>,
            }

            impl From<&Entity> for EntitiesByCreatedAtCursor {
                fn from(entity: &Entity) -> Self {
                    Self {
                        id: entity.id.clone(),
                        created_at: entity.events()
                            .entity_first_persisted_at()
                            .expect("entity not persisted")
                            .clone(),
                    }
                }
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }

    #[test]
    fn list_by_fn() {
        let id_type = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let column = Column::for_id(syn::parse_str("EntityId").unwrap());
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let persist_fn = ListByFn {
            ignore_prefix: None,
            column: &column,
            id: &id_type,
            entity: &entity,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::Soft,
            cursor_mod,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn list_by_id(
                &self,
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
                            "SELECT id FROM entities WHERE (COALESCE(id > $2, true)) AND deleted = FALSE ORDER BY id ASC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT id FROM entities WHERE (COALESCE(id < $2, true)) AND deleted = FALSE ORDER BY id DESC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                };

                let end_cursor = entities.last().map(cursor_mod::EntitiesByIdCursor::from);
                Ok(es_entity::PaginatedQueryRet {
                    entities,
                    has_next_page,
                    end_cursor,
                })
            }

            pub async fn list_by_id_include_deleted(
                &self,
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
                            "SELECT id FROM entities WHERE (COALESCE(id > $2, true)) ORDER BY id ASC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT id FROM entities WHERE (COALESCE(id < $2, true)) ORDER BY id DESC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                        )
                            .fetch_n(first)
                            .await?
                    },
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
    fn list_by_fn_name() {
        let id_type = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let column = Column::new(
            syn::Ident::new("name", proc_macro2::Span::call_site()),
            syn::parse_str("String").unwrap(),
        );
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let persist_fn = ListByFn {
            ignore_prefix: None,
            column: &column,
            id: &id_type,
            entity: &entity,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::No,
            cursor_mod,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn list_by_name(
                &self,
                cursor: es_entity::PaginatedQueryArgs<cursor_mod::EntitiesByNameCursor>,
                direction: es_entity::ListDirection,
            ) -> Result<es_entity::PaginatedQueryRet<Entity, cursor_mod::EntitiesByNameCursor>, es_entity::EsRepoError> {
                let es_entity::PaginatedQueryArgs { first, after } = cursor;
                let (id, name) = if let Some(after) = after {
                    (Some(after.id), Some(after.name))
                } else {
                    (None, None)
                };

                let (entities, has_next_page) = match direction {
                    es_entity::ListDirection::Ascending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT name, id FROM entities WHERE (COALESCE((name, id) > ($3, $2), $2 IS NULL)) ORDER BY name ASC, id ASC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                            name as Option<String>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT name, id FROM entities WHERE (COALESCE((name, id) < ($3, $2), $2 IS NULL)) ORDER BY name DESC, id DESC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                            name as Option<String>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                };

                let end_cursor = entities.last().map(cursor_mod::EntitiesByNameCursor::from);

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
    fn list_by_fn_optional_column() {
        let id_type = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let column = Column::new(
            syn::Ident::new("value", proc_macro2::Span::call_site()),
            syn::parse_str("Option<rust_decimal::Decimal>").unwrap(),
        );
        let cursor_mod = Ident::new("cursor_mod", Span::call_site());

        let persist_fn = ListByFn {
            ignore_prefix: None,
            column: &column,
            id: &id_type,
            entity: &entity,
            table_name: "entities",
            error: &error,
            delete: DeleteOption::No,
            cursor_mod,
            nested_fn_names: Vec::new(),
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn list_by_value(
                &self,
                cursor: es_entity::PaginatedQueryArgs<cursor_mod::EntitiesByValueCursor>,
                direction: es_entity::ListDirection,
            ) -> Result<es_entity::PaginatedQueryRet<Entity, cursor_mod::EntitiesByValueCursor>, es_entity::EsRepoError> {
                let es_entity::PaginatedQueryArgs { first, after } = cursor;
                let (id, value) = if let Some(after) = after {
                    (Some(after.id), after.value)
                } else {
                    (None, None)
                };

                let (entities, has_next_page) = match direction {
                    es_entity::ListDirection::Ascending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT value, id FROM entities WHERE ((value IS NOT DISTINCT FROM $3) AND COALESCE(id > $2, true) OR COALESCE(value > $3, value IS NOT NULL)) ORDER BY value ASC NULLS FIRST, id ASC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                            value as Option<rust_decimal::Decimal>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            self.pool(),
                            "SELECT value, id FROM entities WHERE ((value IS NOT DISTINCT FROM $3) AND COALESCE(id < $2, true) OR COALESCE(value < $3, value IS NOT NULL)) ORDER BY value DESC NULLS LAST, id DESC LIMIT $1",
                            (first + 1) as i64,
                            id as Option<EntityId>,
                            value as Option<rust_decimal::Decimal>,
                        )
                            .fetch_n(first)
                            .await?
                    },
                };

                let end_cursor = entities.last().map(cursor_mod::EntitiesByValueCursor::from);

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
