use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::RepositoryOptions;

pub struct PostPersistHook<'a> {
    event: &'a syn::Ident,
    entity: &'a syn::Ident,
    error: &'a syn::Type,
    hook: &'a Option<syn::Ident>,
}

impl<'a> From<&'a RepositoryOptions> for PostPersistHook<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        Self {
            event: opts.event(),
            entity: opts.entity(),
            error: opts.err(),
            hook: &opts.post_persist_hook,
        }
    }
}

impl<'a> ToTokens for PostPersistHook<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let event = &self.event;
        let entity = &self.entity;
        let error = &self.error;

        let hook = if let Some(hook) = self.hook {
            quote! {
                self.#hook(op, entity, new_events).await?;
                Ok(())
            }
        } else {
            quote! {
                Ok(())
            }
        };

        tokens.append_all(quote! {
            #[inline(always)]
            async fn execute_post_persist_hook(&self,
                op: &mut es_entity::DbOp<'_>,
                entity: &#entity,
                new_events: es_entity::LastPersisted<'_, #event>
            ) -> Result<(), #error> {
                #hook
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_persist_hook() {
        let event = syn::Ident::new("EntityEvent", proc_macro2::Span::call_site());
        let entity = syn::Ident::new("Entity", proc_macro2::Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();
        let hook = None;

        let hook = PostPersistHook {
            event: &event,
            entity: &entity,
            error: &error,
            hook: &hook,
        };

        let mut tokens = TokenStream::new();
        hook.to_tokens(&mut tokens);

        let expected = quote! {
            #[inline(always)]
            async fn execute_post_persist_hook(&self,
                op: &mut es_entity::DbOp<'_>,
                entity: &Entity,
                new_events: es_entity::LastPersisted<'_, #event>
            ) -> Result<(), es_entity::EsRepoError> {
                Ok(())
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
