use darling::{FromDeriveInput, ToTokens};
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

#[derive(Debug, Clone, FromDeriveInput)]
#[darling(attributes(es_event))]
pub struct EsEvent {
    ident: syn::Ident,
    id: syn::Type,
}

pub fn derive(ast: syn::DeriveInput) -> darling::Result<proc_macro2::TokenStream> {
    let event = EsEvent::from_derive_input(&ast)?;
    Ok(quote!(#event))
}

impl ToTokens for EsEvent {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ident = &self.ident;
        let id = &self.id;
        tokens.append_all(quote! {
            impl es_entity::EsEvent for #ident {
                type EntityId = #id;
            }
        });
    }
}
