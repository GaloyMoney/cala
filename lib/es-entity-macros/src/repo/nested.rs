use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::{RepoField, RepositoryOptions};

pub struct Nested<'a> {
    field: &'a RepoField,
    error: &'a syn::Type,
}

impl<'a> Nested<'a> {
    pub fn new(field: &'a RepoField, opts: &'a RepositoryOptions) -> Nested<'a> {
        Nested {
            field,
            error: opts.err(),
        }
    }
}

impl ToTokens for Nested<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let error = self.error;
        let repo_field = self.field.ident();

        let nested_repo_ty = &self.field.ty;
        let create_fn_name = self.field.create_nested_fn_name();
        let update_fn_name = self.field.update_nested_fn_name();
        let find_fn_name = self.field.find_nested_fn_name();

        tokens.append_all(quote! {
            async fn #create_fn_name<P>(&self, op: &mut es_entity::DbOp<'_>, entity: &mut P) -> Result<(), #error>
                where
                    P: es_entity::Parent<<#nested_repo_ty as EsRepo>::Entity>
            {
                let nested = entity.nested_mut();
                if nested.new_entities_mut().is_empty() {
                    return Ok(());
                }

                let mut entities = Vec::new();
                for new_entity in nested.new_entities_mut().drain(..) {
                    let entity = self.#repo_field.create_in_op(op, new_entity).await?;
                    entities.push(entity);
                }
                nested.extend_entities(entities);
                Ok(())
            }

            async fn #update_fn_name<P>(&self, op: &mut es_entity::DbOp<'_>, entity: &mut P) -> Result<(), #error>
                where
                    P: es_entity::Parent<<#nested_repo_ty as EsRepo>::Entity>
            {
                let entities = entity.nested_mut().entities_mut();
                for entity in entities.values_mut() {
                    self.#repo_field.update_in_op(op, entity).await?;
                }
                self.#create_fn_name(op, entity).await?;
                Ok(())
            }

            async fn #find_fn_name<P>(&self, entities: &mut [P]) -> Result<(), #error>
                where
                    P: es_entity::Parent<<#nested_repo_ty as es_entity::EsRepo>::Entity> + es_entity::EsEntity,
                    #nested_repo_ty: es_entity::PopulateNested<<<P as es_entity::EsEntity>::Event as es_entity::EsEvent>::EntityId>,
                    #error: From<<#nested_repo_ty as es_entity::EsRepo>::Err>
            {
                let lookup = entities.iter_mut().map(|e| (e.events().entity_id.clone(), e.nested_mut())).collect();
                self.#repo_field.populate(lookup).await?;
                Ok(())
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::{parse_quote, Ident};

    #[test]
    fn nested() {
        let field = RepoField {
            ident: Some(Ident::new("users", Span::call_site())),
            ty: parse_quote! { UserRepo },
            nested: true,
            pool: false,
        };
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        let cursor = Nested {
            error: &error,
            field: &field,
        };

        let mut tokens = TokenStream::new();
        cursor.to_tokens(&mut tokens);

        let expected = quote! {
            async fn create_nested_users<P>(&self, op: &mut es_entity::DbOp<'_>, entity: &mut P) -> Result<(), es_entity::EsRepoError>
                where
                    P: es_entity::Parent<<UserRepo as EsRepo>::Entity>
            {
                let nested = entity.nested_mut();
                if nested.new_entities_mut().is_empty() {
                    return Ok(());
                }

                let mut entities = Vec::new();
                for new_entity in nested.new_entities_mut().drain(..) {
                    let entity = self.users.create_in_op(op, new_entity).await?;
                    entities.push(entity);
                }
                nested.extend_entities(entities);
                Ok(())
            }

            async fn update_nested_users<P>(&self, op: &mut es_entity::DbOp<'_>, entity: &mut P) -> Result<(), es_entity::EsRepoError>
                where
                    P: es_entity::Parent<<UserRepo as EsRepo>::Entity>
            {
                let entities = entity.nested_mut().entities_mut();
                for entity in entities.values_mut() {
                    self.users.update_in_op(op, entity).await?;
                }
                self.create_nested_users(op, entity).await?;
                Ok(())
            }

            async fn find_nested_users<P>(&self, entities: &mut [P]) -> Result<(), es_entity::EsRepoError>
                where
                    P: es_entity::Parent<<UserRepo as es_entity::EsRepo>::Entity> + es_entity::EsEntity,
                    UserRepo: es_entity::PopulateNested<<<P as es_entity::EsEntity>::Event as es_entity::EsEvent>::EntityId>,
                    es_entity::EsRepoError: From<<UserRepo as es_entity::EsRepo>::Err>
            {
                let lookup = entities.iter_mut().map(|e| (e.events().entity_id.clone(), e.nested_mut())).collect();
                self.users.populate(lookup).await?;
                Ok(())
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
