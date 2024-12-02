#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

mod entity;
mod event;
mod query;
mod repo;
mod retry_on_concurrent_modification;

use proc_macro::TokenStream;
use syn::parse_macro_input;

#[proc_macro_derive(EsEvent, attributes(es_event))]
pub fn es_event_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as syn::DeriveInput);
    match event::derive(ast) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.write_errors().into(),
    }
}

#[proc_macro_attribute]
pub fn retry_on_concurrent_modification(args: TokenStream, input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as syn::ItemFn);
    match retry_on_concurrent_modification::make(args, ast) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.write_errors().into(),
    }
}

#[proc_macro_derive(EsEntity, attributes(es_entity))]
pub fn es_entity_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as syn::DeriveInput);
    match entity::derive(ast) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.write_errors().into(),
    }
}

#[proc_macro_derive(EsRepo, attributes(es_repo))]
pub fn es_repo_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as syn::DeriveInput);
    match repo::derive(ast) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.write_errors().into(),
    }
}

#[proc_macro]
pub fn expand_es_query(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as query::QueryInput);
    match query::expand(input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.write_errors().into(),
    }
}
