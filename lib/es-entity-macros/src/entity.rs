use darling::{FromDeriveInput, FromField, ToTokens};
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};
use syn::Type;

#[derive(Debug, FromField)]
#[darling(attributes(es_entity))]
struct Field {
    ident: Option<syn::Ident>,
    ty: Type,
    #[darling(default)]
    events: bool,
    #[darling(default)]
    nested: bool,
}

impl Field {
    fn is_events_field(&self) -> bool {
        self.events || self.ident.as_ref().map_or(false, |i| i == "events")
    }

    fn extract_nested_entity_type(&self) -> &Type {
        if let Type::Path(type_path) = &self.ty {
            if let Some(segment) = type_path.path.segments.last() {
                if segment.ident == "Nested" {
                    if let syn::PathArguments::AngleBracketed(generic_args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(inner_type)) =
                            generic_args.args.first()
                        {
                            return inner_type;
                        }
                    }
                }
            }
        }
        panic!("Field must be of type Nested<T>");
    }
}

#[derive(Debug, FromDeriveInput)]
#[darling(supports(struct_named), attributes(es_event))]
pub struct EsEntity {
    ident: syn::Ident,
    #[darling(default, rename = "new")]
    new_entity_ident: Option<syn::Ident>,
    #[darling(default, rename = "event")]
    event_ident: Option<syn::Ident>,
    data: darling::ast::Data<(), Field>,
}

impl EsEntity {
    fn find_events_field(&self) -> Option<&Field> {
        match &self.data {
            darling::ast::Data::Struct(fields) => {
                fields.iter().find(|field| field.is_events_field())
            }
            _ => None,
        }
    }

    fn nested_fields(&self) -> Vec<&Field> {
        match &self.data {
            darling::ast::Data::Struct(fields) => {
                fields.iter().filter(|field| field.nested).collect()
            }
            _ => Vec::new(),
        }
    }
}

pub fn derive(ast: syn::DeriveInput) -> darling::Result<proc_macro2::TokenStream> {
    let entity = EsEntity::from_derive_input(&ast)?;
    Ok(quote!(#entity))
}

impl ToTokens for EsEntity {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ident = &self.ident;
        let events_field = self
            .find_events_field()
            .expect("Struct must have a field marked with #[es_entity(events)]")
            .ident
            .as_ref()
            .expect("Not ident on #[events]");

        let event = self.event_ident.clone().unwrap_or_else(|| {
            syn::Ident::new(
                &format!("{}Event", self.ident),
                proc_macro2::Span::call_site(),
            )
        });
        let new = self.new_entity_ident.clone().unwrap_or_else(|| {
            syn::Ident::new(
                &format!("New{}", self.ident),
                proc_macro2::Span::call_site(),
            )
        });

        let nested = self.nested_fields().into_iter().map(|f| {
            let field = &f.ident;
            let ty = f.extract_nested_entity_type();
            quote! {
                impl es_entity::Parent<#ty> for #ident {
                    fn nested(&self) -> &es_entity::Nested<#ty> {
                        &self.#field
                    }
                    fn nested_mut(&mut self) -> &mut es_entity::Nested<#ty> {
                        &mut self.#field
                    }
                }
            }
        });

        tokens.append_all(quote! {
            impl es_entity::EsEntity for #ident {
                type Event = #event;
                type New = #new;

                fn events_mut(&mut self) -> &mut es_entity::EntityEvents<#event> {
                    &mut self.#events_field
                }
                fn events(&self) -> &es_entity::EntityEvents<#event> {
                    &self.#events_field
                }
            }

            #(#nested)*
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn test_derive_es_entity() {
        let input: syn::DeriveInput = parse_quote! {
            #[derive(EsEntity)]
            pub struct User {
                pub id: UserId,
                pub email: String,
                #[es_entity(events)]
                the_events: EntityEvents<UserEvent>
            }
        };

        let output = derive(input).unwrap();
        let expected = quote! {
            impl es_entity::EsEntity for User {
                type Event = UserEvent;
                type New = NewUser;
                fn events_mut(&mut self) -> &mut es_entity::EntityEvents<UserEvent> {
                    &mut self.the_events
                }
                fn events(&self) -> &es_entity::EntityEvents<UserEvent> {
                    &self.the_events
                }
            }
        };

        assert_eq!(output.to_string(), expected.to_string());
    }

    #[test]
    fn test_derive_without_events_attr() {
        let input: syn::DeriveInput = parse_quote! {
            #[derive(EsEntity)]
            pub struct User {
                pub id: UserId,
                events: EntityEvents<UserEvent>
            }
        };

        let output = derive(input).unwrap();
        let expected = quote! {
            impl es_entity::EsEntity for User {
                type Event = UserEvent;
                type New = NewUser;
                fn events_mut(&mut self) -> &mut es_entity::EntityEvents<UserEvent> {
                    &mut self.events
                }
                fn events(&self) -> &es_entity::EntityEvents<UserEvent> {
                    &self.events
                }
            }
        };

        assert_eq!(output.to_string(), expected.to_string());
    }

    #[test]
    fn test_derive_with_nested() {
        let input: syn::DeriveInput = parse_quote! {
            #[derive(EsEntity)]
            pub struct User {
                pub id: UserId,
                #[es_entity(nested)]
                children: Nested<ChildEntity>,
                events: EntityEvents<UserEvent>
            }
        };

        let output = derive(input).unwrap();
        let expected = quote! {
            impl es_entity::EsEntity for User {
                type Event = UserEvent;
                type New = NewUser;
                fn events_mut(&mut self) -> &mut es_entity::EntityEvents<UserEvent> {
                    &mut self.events
                }
                fn events(&self) -> &es_entity::EntityEvents<UserEvent> {
                    &self.events
                }
            }

            impl es_entity::Parent<ChildEntity> for User {
                fn nested(&self) -> &es_entity::Nested<ChildEntity> {
                    &self.children
                }
                fn nested_mut(&mut self) -> &mut es_entity::Nested<ChildEntity> {
                    &mut self.children
                }
            }
        };

        assert_eq!(output.to_string(), expected.to_string());
    }
}
