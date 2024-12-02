mod begin;
mod combo_cursor;
mod create_all_fn;
mod create_fn;
mod delete_fn;
mod find_all_fn;
mod find_by_fn;
mod find_many;
mod list_by_fn;
mod list_for_fn;
mod nested;
mod options;
mod persist_events_batch_fn;
mod persist_events_fn;
mod populate_nested;
mod post_persist_hook;
mod update_fn;

use darling::{FromDeriveInput, ToTokens};
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use options::RepositoryOptions;

pub fn derive(ast: syn::DeriveInput) -> darling::Result<proc_macro2::TokenStream> {
    let opts = RepositoryOptions::from_derive_input(&ast)?;
    let repo = EsRepo::from(&opts);
    Ok(quote!(#repo))
}
pub struct EsRepo<'a> {
    repo: &'a syn::Ident,
    persist_events_fn: persist_events_fn::PersistEventsFn<'a>,
    persist_events_batch_fn: persist_events_batch_fn::PersistEventsBatchFn<'a>,
    update_fn: update_fn::UpdateFn<'a>,
    create_fn: create_fn::CreateFn<'a>,
    create_all_fn: create_all_fn::CreateAllFn<'a>,
    delete_fn: delete_fn::DeleteFn<'a>,
    find_by_fns: Vec<find_by_fn::FindByFn<'a>>,
    find_all_fn: find_all_fn::FindAllFn<'a>,
    post_persist_hook: post_persist_hook::PostPersistHook<'a>,
    begin: begin::Begin<'a>,
    list_by_fns: Vec<list_by_fn::ListByFn<'a>>,
    list_for_fns: Vec<list_for_fn::ListForFn<'a>>,
    nested: Vec<nested::Nested<'a>>,
    populate_nested: Option<populate_nested::PopulateNested<'a>>,
    opts: &'a RepositoryOptions,
}

impl<'a> From<&'a RepositoryOptions> for EsRepo<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        let find_by_fns = opts
            .columns
            .all_find_by()
            .map(|c| find_by_fn::FindByFn::new(c, opts))
            .collect();
        let list_by_fns = opts
            .columns
            .all_list_by()
            .map(|c| list_by_fn::ListByFn::new(c, opts))
            .collect();
        let list_for_fns = opts
            .columns
            .all_list_for()
            .flat_map(|list_for_column| {
                opts.columns
                    .all_list_by()
                    .map(|b| list_for_fn::ListForFn::new(list_for_column, b, opts))
            })
            .collect();
        let populate_nested = opts
            .columns
            .parent()
            .map(|c| populate_nested::PopulateNested::new(c, opts));
        let nested = opts
            .all_nested()
            .map(|n| nested::Nested::new(n, opts))
            .collect();

        Self {
            repo: &opts.ident,
            persist_events_fn: persist_events_fn::PersistEventsFn::from(opts),
            persist_events_batch_fn: persist_events_batch_fn::PersistEventsBatchFn::from(opts),
            update_fn: update_fn::UpdateFn::from(opts),
            create_fn: create_fn::CreateFn::from(opts),
            create_all_fn: create_all_fn::CreateAllFn::from(opts),
            delete_fn: delete_fn::DeleteFn::from(opts),
            find_by_fns,
            find_all_fn: find_all_fn::FindAllFn::from(opts),
            post_persist_hook: post_persist_hook::PostPersistHook::from(opts),
            begin: begin::Begin::from(opts),
            list_by_fns,
            list_for_fns,
            nested,
            populate_nested,
            opts,
        }
    }
}

impl<'a> ToTokens for EsRepo<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let repo = &self.repo;
        let persist_events_fn = &self.persist_events_fn;
        let persist_events_batch_fn = &self.persist_events_batch_fn;
        let update_fn = &self.update_fn;
        let create_fn = &self.create_fn;
        let create_all_fn = &self.create_all_fn;
        let delete_fn = &self.delete_fn;
        let find_by_fns = &self.find_by_fns;
        let find_all_fn = &self.find_all_fn;
        let post_persist_hook = &self.post_persist_hook;
        let begin = &self.begin;
        let cursors = self.list_by_fns.iter().map(|l| l.cursor());
        let combo_cursor = combo_cursor::ComboCursor::new(
            self.opts,
            self.list_by_fns.iter().map(|l| l.cursor()).collect(),
        );
        let sort_by = combo_cursor.sort_by();
        let find_many = find_many::FindManyFn::new(
            self.opts,
            &self.list_for_fns,
            self.opts.columns.all_list_for().collect(),
            self.opts.columns.all_list_by().collect(),
            &combo_cursor,
        );
        let find_many_filter = &find_many.filter;
        #[cfg(feature = "graphql")]
        let gql_combo_cursor = combo_cursor.gql_cursor();
        #[cfg(not(feature = "graphql"))]
        let gql_combo_cursor = TokenStream::new();
        #[cfg(feature = "graphql")]
        let gql_cursors: Vec<_> = self
            .list_by_fns
            .iter()
            .map(|l| l.cursor().gql_cursor())
            .collect();
        #[cfg(not(feature = "graphql"))]
        let gql_cursors: Vec<TokenStream> = Vec::new();
        let list_by_fns = &self.list_by_fns;
        let list_for_fns = &self.list_for_fns;

        let entity = self.opts.entity();
        let event = self.opts.event();
        let id = self.opts.id();
        let error = self.opts.err();

        let cursor_mod = self.opts.cursor_mod();
        let types_mod = self.opts.repo_types_mod();

        let nested = &self.nested;
        let populate_nested = &self.populate_nested;
        let pool_field = self.opts.pool_field();

        tokens.append_all(quote! {
            pub mod #cursor_mod {
                use super::*;

                #(#cursors)*
                #(#gql_cursors)*

                #combo_cursor
                #gql_combo_cursor
            }

            mod #types_mod {

                use super::*;

                #[allow(non_camel_case_types)]
                pub(super) type Repo__Id = #id;
                #[allow(non_camel_case_types)]
                pub(super) type Repo__Event = #event;
                #[allow(non_camel_case_types)]
                pub(super) type Repo__Entity = #entity;
                #[allow(non_camel_case_types)]
                pub(super) type Repo__Error = #error;
                #[allow(non_camel_case_types)]
                pub(super) type Repo__DbEvent = es_entity::GenericEvent<#id>;

                pub(super) struct QueryRes {
                    pub(super) rows: Vec<Repo__DbEvent>,
                }

                impl QueryRes {
                    pub(super) async fn fetch_one(
                        self,
                    ) -> Result<Repo__Entity, Repo__Error>
                    {
                        Ok(es_entity::EntityEvents::load_first(self.rows.into_iter())?)
                    }

                    pub(super) async fn fetch_n(
                        self,
                        first: usize,
                    ) -> Result<(Vec<Repo__Entity>, bool), Repo__Error>
                    {
                        Ok(es_entity::EntityEvents::load_n(self.rows.into_iter(), first)?)
                    }
                }
            }

            #find_many_filter
            #sort_by

            impl #repo {
                #[inline(always)]
                pub fn pool(&self) -> &sqlx::PgPool {
                    &self.#pool_field
                }

                #begin
                #post_persist_hook
                #persist_events_fn
                #persist_events_batch_fn
                #create_fn
                #create_all_fn
                #update_fn
                #delete_fn
                #(#find_by_fns)*
                #find_all_fn
                #find_many
                #(#list_by_fns)*
                #(#list_for_fns)*

                #(#nested)*
            }

            #populate_nested

            impl es_entity::EsRepo for #repo {
                type Entity = #entity;
                type Err = #error;
            }
        });
    }
}
