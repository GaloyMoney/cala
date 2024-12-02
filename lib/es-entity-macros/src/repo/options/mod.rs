mod columns;
mod delete;

use convert_case::{Case, Casing};
use darling::{FromDeriveInput, FromField};

pub use columns::*;
pub use delete::*;

#[derive(FromField)]
#[darling(attributes(es_repo))]
pub struct RepoField {
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,
    #[darling(default)]
    pub pool: bool,
    #[darling(default)]
    pub nested: bool,
}

impl RepoField {
    pub fn ident(&self) -> &syn::Ident {
        self.ident.as_ref().expect("Field must have an identifier")
    }

    fn is_pool_field(&self) -> bool {
        self.pool || self.ident.as_ref().map_or(false, |i| i == "pool")
    }

    pub fn create_nested_fn_name(&self) -> syn::Ident {
        syn::Ident::new(
            &format!("create_nested_{}", self.ident()),
            proc_macro2::Span::call_site(),
        )
    }

    pub fn update_nested_fn_name(&self) -> syn::Ident {
        syn::Ident::new(
            &format!("update_nested_{}", self.ident()),
            proc_macro2::Span::call_site(),
        )
    }

    pub fn find_nested_fn_name(&self) -> syn::Ident {
        syn::Ident::new(
            &format!("find_nested_{}", self.ident()),
            proc_macro2::Span::call_site(),
        )
    }
}

#[derive(FromDeriveInput)]
#[darling(attributes(es_repo), map = "Self::update_defaults")]
pub struct RepositoryOptions {
    pub ident: syn::Ident,
    #[darling(default)]
    pub columns: Columns,
    #[darling(default)]
    pub post_persist_hook: Option<syn::Ident>,
    #[darling(default)]
    pub begin: Option<syn::Ident>,
    #[darling(default)]
    pub delete: DeleteOption,

    data: darling::ast::Data<(), RepoField>,

    #[darling(rename = "entity")]
    entity_ident: syn::Ident,
    #[darling(default, rename = "event")]
    event_ident: Option<syn::Ident>,
    #[darling(default, rename = "id")]
    id_ty: Option<syn::Ident>,
    #[darling(default, rename = "err")]
    err_ty: Option<syn::Type>,
    #[darling(default, rename = "tbl_prefix")]
    prefix: Option<syn::LitStr>,
    #[darling(default, rename = "tbl")]
    table_name: Option<String>,
    #[darling(default, rename = "events_tbl")]
    events_table_name: Option<String>,
}

impl RepositoryOptions {
    fn update_defaults(mut self) -> Self {
        let entity_name = self.entity_ident.to_string();
        if self.event_ident.is_none() {
            self.event_ident = Some(syn::Ident::new(
                &format!("{}Event", entity_name),
                proc_macro2::Span::call_site(),
            ));
        }
        if self.id_ty.is_none() {
            self.id_ty = Some(syn::Ident::new(
                &format!("{}Id", entity_name),
                proc_macro2::Span::call_site(),
            ));
        }
        if self.err_ty.is_none() {
            self.err_ty =
                Some(syn::parse_str("es_entity::EsRepoError").expect("Failed to parse error type"));
        }
        let prefix = if let Some(prefix) = &self.prefix {
            format!("{}_", prefix.value())
        } else {
            String::new()
        };
        if self.table_name.is_none() {
            self.table_name = Some(format!(
                "{prefix}{}",
                pluralizer::pluralize(&entity_name, 2, false).to_case(Case::Snake)
            ));
        }
        if self.events_table_name.is_none() {
            self.events_table_name =
                Some(format!("{prefix}{}Events", entity_name).to_case(Case::Snake));
        }

        self.columns
            .set_id_column(self.id_ty.as_ref().expect("Id not set"));

        self
    }

    pub fn entity(&self) -> &syn::Ident {
        &self.entity_ident
    }

    pub fn table_name(&self) -> &str {
        self.table_name.as_ref().expect("Table name is not set")
    }

    pub fn table_prefix(&self) -> Option<&syn::LitStr> {
        self.prefix.as_ref()
    }

    pub fn id(&self) -> &syn::Ident {
        self.id_ty.as_ref().expect("ID identifier is not set")
    }

    pub fn event(&self) -> &syn::Ident {
        self.event_ident
            .as_ref()
            .expect("Event identifier is not set")
    }

    pub fn events_table_name(&self) -> &str {
        self.events_table_name
            .as_ref()
            .expect("Events table name is not set")
    }

    pub fn cursor_mod(&self) -> syn::Ident {
        let name = format!("{}Cursor", self.entity_ident).to_case(Case::Snake);
        syn::Ident::new(&name, proc_macro2::Span::call_site())
    }

    pub fn repo_types_mod(&self) -> syn::Ident {
        let name = format!("{}RepoTypes", self.entity_ident).to_case(Case::Snake);
        syn::Ident::new(&name, proc_macro2::Span::call_site())
    }

    pub fn pool_field(&self) -> &syn::Ident {
        let field = match &self.data {
            darling::ast::Data::Struct(fields) => fields.iter().find_map(|field| {
                if field.is_pool_field() {
                    Some(field.ident.as_ref().unwrap())
                } else {
                    None
                }
            }),
            _ => None,
        };
        field.expect("Repo must have a field named 'pool' or marked with #[es_repo(pool)]")
    }

    pub fn all_nested(&self) -> impl Iterator<Item = &RepoField> {
        if let darling::ast::Data::Struct(fields) = &self.data {
            fields.iter().filter(|f| f.nested)
        } else {
            panic!("Repository must be a struct")
        }
    }

    pub fn err(&self) -> &syn::Type {
        self.err_ty.as_ref().expect("Error identifier is not set")
    }
}
