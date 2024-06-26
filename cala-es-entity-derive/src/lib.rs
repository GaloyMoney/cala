use proc_macro::TokenStream;
use quote::quote;
use syn;

#[proc_macro_derive(EsEntity)]
pub fn es_entity_derive(input: TokenStream) -> TokenStream {
    let ast = syn::parse(input).unwrap();

    impl_es_entity_macro(&ast)
}

fn impl_es_entity_macro(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl #name {
            fn event_table_name() -> &'static str {
                stringify!(#name)
            }
        }
    };
    gen.into()
}
