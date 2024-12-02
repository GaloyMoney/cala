use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::RepositoryOptions;

pub struct Begin<'a> {
    begin: &'a Option<syn::Ident>,
}

impl<'a> From<&'a RepositoryOptions> for Begin<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        Self { begin: &opts.begin }
    }
}

impl<'a> ToTokens for Begin<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let begin = if let Some(begin) = self.begin {
            quote! {
                self.#begin()
            }
        } else {
            quote! {
                es_entity::DbOp::init(self.pool()).await
            }
        };

        tokens.append_all(quote! {
            #[inline(always)]
            pub async fn begin_op(&self) -> Result<es_entity::DbOp<'static>, sqlx::Error>{
                #begin
            }
        });
    }
}
