use darling::FromMeta;
use quote::quote;

#[derive(Default)]
pub struct Columns {
    all: Vec<Column>,
}

impl Columns {
    #[cfg(test)]
    pub fn new(id: &syn::Ident, columns: impl IntoIterator<Item = Column>) -> Self {
        let all = columns.into_iter().collect();
        let mut res = Columns { all };
        res.set_id_column(id);
        res
    }

    pub fn set_id_column(&mut self, ty: &syn::Ident) {
        let mut all = vec![
            Column::for_created_at(),
            Column::for_id(syn::parse_str(&ty.to_string()).unwrap()),
        ];
        all.append(&mut self.all);
        self.all = all;
    }

    pub fn all_find_by(&self) -> impl Iterator<Item = &Column> {
        self.all.iter().filter(|c| c.opts.find_by())
    }

    pub fn all_list_by(&self) -> impl Iterator<Item = &Column> {
        self.all.iter().filter(|c| c.opts.list_by())
    }

    pub fn all_list_for(&self) -> impl Iterator<Item = &Column> {
        self.all.iter().filter(|c| c.opts.list_for())
    }

    pub fn parent(&self) -> Option<&Column> {
        self.all.iter().find(|c| c.opts.parent)
    }

    pub fn updates_needed(&self) -> bool {
        self.all.iter().any(|c| c.opts.persist_on_update())
    }

    pub fn variable_assignments_for_update(&self, ident: syn::Ident) -> proc_macro2::TokenStream {
        let assignments = self.all.iter().filter_map(|c| {
            if c.opts.persist_on_update() || c.opts.is_id {
                Some(c.variable_assignment_for_update(&ident))
            } else {
                None
            }
        });
        quote! {
            #(#assignments)*
        }
    }

    pub fn variable_assignments_for_create(&self, ident: syn::Ident) -> proc_macro2::TokenStream {
        let assignments = self.all.iter().filter_map(|c| {
            if c.opts.persist_on_create() {
                Some(c.variable_assignment_for_create(&ident))
            } else {
                None
            }
        });
        quote! {
            #(#assignments)*
        }
    }

    pub fn variable_assignments_for_create_all(
        &self,
        ident: syn::Ident,
    ) -> proc_macro2::TokenStream {
        let assignments = self.all.iter().filter_map(|c| {
            if c.opts.persist_on_create() {
                Some(c.variable_assignment_for_create_all(&ident))
            } else {
                None
            }
        });
        quote! {
            #(#assignments)*
        }
    }

    pub fn create_query_args(&self) -> Vec<proc_macro2::TokenStream> {
        self.all
            .iter()
            .filter(|c| c.opts.persist_on_create())
            .map(|column| {
                let ident = &column.name;
                let ty = &column.opts.ty;
                quote! {
                    #ident as &#ty,
                }
            })
            .collect()
    }

    pub fn create_query_builder_args(&self) -> Vec<proc_macro2::TokenStream> {
        self.all
            .iter()
            .filter(|c| c.opts.persist_on_create())
            .map(|column| {
                let ident = &column.name;
                quote! {
                    builder.push_bind(#ident);
                }
            })
            .collect()
    }

    pub fn insert_column_names(&self) -> Vec<String> {
        self.all
            .iter()
            .filter_map(|c| {
                if c.opts.persist_on_create() {
                    Some(c.name.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn insert_placeholders(&self) -> String {
        let count = self
            .all
            .iter()
            .filter(|c| c.opts.persist_on_create())
            .count();
        (1..=count)
            .map(|i| format!("${}", i))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn sql_updates(&self) -> String {
        self.all
            .iter()
            .skip(1)
            .filter(|c| c.opts.persist_on_update())
            .enumerate()
            .map(|(idx, column)| format!("{} = ${}", column.name, idx + 2))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn update_query_args(&self) -> Vec<proc_macro2::TokenStream> {
        self.all
            .iter()
            .filter(|c| c.opts.persist_on_update() || c.opts.is_id)
            .map(|column| {
                let ident = &column.name;
                let ty = &column.opts.ty;
                quote! {
                    #ident as &#ty
                }
            })
            .collect()
    }
}

impl FromMeta for Columns {
    fn from_list(items: &[darling::ast::NestedMeta]) -> darling::Result<Self> {
        let all = items
            .iter()
            .map(Column::from_nested_meta)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Columns { all })
    }
}

#[derive(PartialEq)]
pub struct Column {
    name: syn::Ident,
    opts: ColumnOpts,
}

impl FromMeta for Column {
    fn from_nested_meta(item: &darling::ast::NestedMeta) -> darling::Result<Self> {
        match item {
            darling::ast::NestedMeta::Meta(
                meta @ syn::Meta::NameValue(syn::MetaNameValue {
                    value:
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(ref lit_str),
                            ..
                        }),
                    ..
                }),
            ) => {
                let name = meta.path().get_ident().cloned().ok_or_else(|| {
                    darling::Error::custom("Expected identifier").with_span(meta.path())
                })?;
                Ok(Column::new(name, syn::parse_str(&lit_str.value())?))
            }
            darling::ast::NestedMeta::Meta(meta @ syn::Meta::List(_)) => {
                let name = meta.path().get_ident().cloned().ok_or_else(|| {
                    darling::Error::custom("Expected identifier").with_span(meta.path())
                })?;
                let column = Column {
                    name,
                    opts: ColumnOpts::from_meta(meta)?,
                };
                Ok(column)
            }
            _ => Err(
                darling::Error::custom("Expected name-value pair or attribute list")
                    .with_span(item),
            ),
        }
    }
}

impl Column {
    pub fn new(name: syn::Ident, ty: syn::Type) -> Self {
        Column {
            name,
            opts: ColumnOpts::new(ty),
        }
    }

    pub fn for_id(ty: syn::Type) -> Self {
        Column {
            name: syn::Ident::new("id", proc_macro2::Span::call_site()),
            opts: ColumnOpts {
                ty,
                is_id: true,
                list_by: Some(true),
                find_by: Some(true),
                list_for: Some(false),
                parent: false,
                create_opts: Some(CreateOpts {
                    persist: Some(true),
                    accessor: None,
                }),
                update_opts: Some(UpdateOpts {
                    persist: Some(false),
                    accessor: None,
                }),
            },
        }
    }

    pub fn for_created_at() -> Self {
        Column {
            name: syn::Ident::new("created_at", proc_macro2::Span::call_site()),
            opts: ColumnOpts {
                ty: syn::parse_quote!(
                    es_entity::prelude::chrono::DateTime<es_entity::prelude::chrono::Utc>
                ),
                is_id: false,
                list_by: Some(true),
                find_by: Some(false),
                list_for: Some(false),
                parent: false,
                create_opts: Some(CreateOpts {
                    persist: Some(false),
                    accessor: None,
                }),
                update_opts: Some(UpdateOpts {
                    persist: Some(false),
                    accessor: Some(syn::parse_quote!(events()
                        .entity_first_persisted_at()
                        .expect("entity not persisted"))),
                }),
            },
        }
    }

    pub fn is_id(&self) -> bool {
        self.opts.is_id
    }

    pub fn is_optional(&self) -> bool {
        if let syn::Type::Path(type_path) = self.ty() {
            if type_path.path.segments.len() == 1 {
                let segment = &type_path.path.segments[0];
                if segment.ident == "Option" {
                    return true;
                }
            }
        }
        false
    }

    pub fn name(&self) -> &syn::Ident {
        &self.name
    }

    pub fn ty(&self) -> &syn::Type {
        &self.opts.ty
    }

    pub fn accessor(&self) -> proc_macro2::TokenStream {
        self.opts.update_accessor(&self.name)
    }

    fn variable_assignment_for_create(&self, ident: &syn::Ident) -> proc_macro2::TokenStream {
        let name = &self.name;
        let accessor = self.opts.create_accessor(name);
        quote! {
            let #name = &#ident.#accessor;
        }
    }

    fn variable_assignment_for_create_all(&self, ident: &syn::Ident) -> proc_macro2::TokenStream {
        let name = &self.name;
        let accessor = self.opts.create_accessor(name);
        let needs_ref = self
            .opts
            .create_opts
            .as_ref()
            .map(|o| o.accessor.is_none())
            .unwrap_or(true);
        let ty = &self.opts.ty;
        if needs_ref {
            quote! {
                let #name: &#ty = &#ident.#accessor;
            }
        } else {
            quote! {
                let #name: #ty = #ident.#accessor;
            }
        }
    }

    fn variable_assignment_for_update(&self, ident: &syn::Ident) -> proc_macro2::TokenStream {
        let name = &self.name;
        let accessor = self.opts.update_accessor(name);
        quote! {
            let #name = &#ident.#accessor;
        }
    }
}

#[derive(PartialEq, FromMeta)]
struct ColumnOpts {
    ty: syn::Type,
    #[darling(default, skip)]
    is_id: bool,
    #[darling(default)]
    find_by: Option<bool>,
    #[darling(default)]
    list_by: Option<bool>,
    #[darling(default)]
    list_for: Option<bool>,
    #[darling(default)]
    parent: bool,
    #[darling(default, rename = "create")]
    create_opts: Option<CreateOpts>,
    #[darling(default, rename = "update")]
    update_opts: Option<UpdateOpts>,
}

impl ColumnOpts {
    fn new(ty: syn::Type) -> Self {
        ColumnOpts {
            ty,
            is_id: false,
            find_by: None,
            list_by: None,
            list_for: None,
            parent: false,
            create_opts: None,
            update_opts: None,
        }
    }

    fn find_by(&self) -> bool {
        self.find_by.unwrap_or(true)
    }

    fn list_by(&self) -> bool {
        self.list_by.unwrap_or(!self.list_for())
    }

    fn list_for(&self) -> bool {
        self.list_for.unwrap_or(false)
    }

    fn persist_on_create(&self) -> bool {
        self.create_opts
            .as_ref()
            .map_or(true, |o| o.persist.unwrap_or(true))
    }

    fn create_accessor(&self, name: &syn::Ident) -> proc_macro2::TokenStream {
        if let Some(accessor) = &self.create_opts.as_ref().and_then(|o| o.accessor.as_ref()) {
            quote! {
                #accessor
            }
        } else {
            quote! {
                #name
            }
        }
    }

    fn persist_on_update(&self) -> bool {
        self.update_opts
            .as_ref()
            .map_or(true, |o| o.persist.unwrap_or(true))
    }

    fn update_accessor(&self, name: &syn::Ident) -> proc_macro2::TokenStream {
        if let Some(accessor) = &self.update_opts.as_ref().and_then(|o| o.accessor.as_ref()) {
            quote! {
                #accessor
            }
        } else {
            quote! {
                #name
            }
        }
    }
}

#[derive(Default, PartialEq, FromMeta)]
struct CreateOpts {
    persist: Option<bool>,
    accessor: Option<syn::Expr>,
}

#[derive(Default, PartialEq, FromMeta)]
struct UpdateOpts {
    persist: Option<bool>,
    accessor: Option<syn::Expr>,
}

#[cfg(test)]
mod tests {
    use darling::FromMeta;
    use syn::parse_quote;

    use super::*;

    #[test]
    fn column_opts_from_list() {
        let input: syn::Meta = parse_quote!(thing(
            ty = "crate::module::Thing",
            list_by = false,
            create(persist = true, accessor = accessor_fn()),
        ));
        let values = ColumnOpts::from_meta(&input).expect("Failed to parse Field");
        assert_eq!(values.ty, parse_quote!(crate::module::Thing));
        assert!(!values.list_by());
        assert!(values.find_by());
        // assert!(values.update());
        assert_eq!(
            values.create_opts.unwrap().accessor.unwrap(),
            parse_quote!(accessor_fn())
        );
    }

    #[test]
    fn columns_from_list() {
        let input: syn::Meta = parse_quote!(columns(
            name = "String",
            email(
                ty = "String",
                list_by = false,
                create(accessor = "email()"),
                update(persist = false)
            )
        ));
        let columns = Columns::from_meta(&input).expect("Failed to parse Fields");
        assert_eq!(columns.all.len(), 2);

        assert_eq!(columns.all[0].name.to_string(), "name");

        assert_eq!(columns.all[1].name.to_string(), "email");
        assert!(!columns.all[1].opts.list_by());
        assert_eq!(
            columns.all[1]
                .opts
                .create_accessor(&parse_quote!(email))
                .to_string(),
            quote!(email()).to_string()
        );
        assert!(!columns.all[1].opts.persist_on_update());
    }
}
