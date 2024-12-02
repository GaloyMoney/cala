use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct PopulateNested<'a> {
    column: &'a Column,
    ident: &'a syn::Ident,
    error: &'a syn::Type,
    id: &'a syn::Ident,
    table_name: &'a str,
    events_table_name: &'a str,
    repo_types_mod: syn::Ident,
}

impl<'a> PopulateNested<'a> {
    pub fn new(column: &'a Column, opts: &'a RepositoryOptions) -> Self {
        Self {
            column,
            ident: &opts.ident,
            error: opts.err(),
            id: opts.id(),
            table_name: opts.table_name(),
            events_table_name: opts.events_table_name(),
            repo_types_mod: opts.repo_types_mod(),
        }
    }
}

impl<'a> ToTokens for PopulateNested<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ty = self.column.ty();
        let ident = self.ident;
        let error = self.error;
        let repo_types_mod = &self.repo_types_mod;
        let column_name = self.column.name();

        let query = format!(
            r#"WITH entities AS (SELECT * FROM {} WHERE ({} = ANY($1))) SELECT i.id AS "entity_id: {}", e.sequence, e.event, e.recorded_at FROM entities i JOIN {} e ON i.id = e.id ORDER BY e.id, e.sequence"#,
            self.table_name,
            self.column.name(),
            self.id,
            self.events_table_name,
        );

        tokens.append_all(quote! {
            #[es_entity::prelude::async_trait::async_trait]
            impl es_entity::PopulateNested<#ty> for #ident {
                async fn populate(
                    &self,
                    mut lookup: std::collections::HashMap<#ty, &mut Nested<<Self as EsRepo>::Entity>>,
                ) -> Result<(), #error> {
                    let parent_ids: Vec<_> = lookup.keys().collect();
                    let rows = sqlx::query_as!(
                        #repo_types_mod::Repo__DbEvent,
                        #query,
                        parent_ids.as_slice() as &[&#ty],
                    )
                        .fetch_all(self.pool())
                        .await?;
                    let n = rows.len();
                    let (res, _) = es_entity::EntityEvents::load_n::<<Self as EsRepo>::Entity>(rows.into_iter(), n)?;
                    for entity in res.into_iter() {
                        let parent = lookup.get_mut(&entity.#column_name).expect("parent not present");
                        parent.extend_entities(std::iter::once(entity));
                    }
                    Ok(())
                }
            }
        });
    }
}
