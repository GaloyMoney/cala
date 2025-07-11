use darling::ToTokens;
use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};

use super::options::*;

pub struct FindAllFn<'a> {
    id: &'a syn::Ident,
    entity: &'a syn::Ident,
    table_name: &'a str,
    events_table_name: &'a str,
    error: &'a syn::Type,
}

impl<'a> From<&'a RepositoryOptions> for FindAllFn<'a> {
    fn from(opts: &'a RepositoryOptions) -> Self {
        Self {
            id: opts.id(),
            entity: opts.entity(),
            table_name: opts.table_name(),
            events_table_name: opts.events_table_name(),
            error: opts.err(),
        }
    }
}

impl ToTokens for FindAllFn<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = self.id;
        let entity = self.entity;
        let error = self.error;

        let query = format!(
            "SELECT i.id, e.sequence, e.event, e.recorded_at \
             FROM {} i \
             JOIN {} e ON i.id = e.id \
             WHERE i.id = ANY($1) \
             ORDER BY i.id, e.sequence",
            self.table_name, self.events_table_name
        );

        tokens.append_all(quote! {
            pub async fn find_all<Out: From<#entity>>(
                &self,
                ids: &[#id]
            ) -> Result<std::collections::HashMap<#id, Out>, #error> {
                self.find_all_via(self.pool(), ids).await
            }

            pub async fn find_all_in_tx<Out: From<#entity>>(
                &self,
                db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                ids: &[#id]
            ) -> Result<std::collections::HashMap<#id, Out>, #error> {
                self.find_all_via(&mut **db, ids).await
            }

            async fn find_all_via<Out: From<#entity>>(
                &self,
                executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
                ids: &[#id]
            ) -> Result<std::collections::HashMap<#id, Out>, #error> {
                #[derive(sqlx::FromRow)]
                struct EventRow {
                    id: #id,
                    sequence: i32,
                    event: es_entity::prelude::serde_json::Value,
                    recorded_at: es_entity::prelude::chrono::DateTime<es_entity::prelude::chrono::Utc>,
                }

                let rows: Vec<EventRow> = sqlx::query_as(#query)
                    .bind(ids)
                    .fetch_all(executor)
                    .await?;

                let n = rows.len();
                let res = es_entity::EntityEvents::load_n::<#entity>(rows.into_iter().map(|r|
                        es_entity::GenericEvent {
                            entity_id: r.id,
                            sequence: r.sequence,
                            event: r.event,
                            recorded_at: r.recorded_at,
                        }), n)?;

                Ok(res.0.into_iter().map(|u| (u.id.clone(), Out::from(u))).collect())
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::Ident;

    #[test]
    fn find_all_fn() {
        let id_type = Ident::new("EntityId", Span::call_site());
        let entity = Ident::new("Entity", Span::call_site());
        let error = syn::parse_str("es_entity::EsRepoError").unwrap();

        let persist_fn = FindAllFn {
            id: &id_type,
            entity: &entity,
            table_name: "entities",
            events_table_name: "entity_events",
            error: &error,
        };

        let mut tokens = TokenStream::new();
        persist_fn.to_tokens(&mut tokens);

        let expected = quote! {
            pub async fn find_all<Out: From<Entity>>(
                &self,
                ids: &[EntityId]
            ) -> Result<std::collections::HashMap<EntityId, Out>, es_entity::EsRepoError> {
                self.find_all_via(self.pool(), ids).await
            }

            pub async fn find_all_in_tx<Out: From<Entity>>(
                &self,
                db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                ids: &[EntityId]
            ) -> Result<std::collections::HashMap<EntityId, Out>, es_entity::EsRepoError> {
                self.find_all_via(&mut **db, ids).await
            }

            async fn find_all_via<Out: From<Entity>>(
                &self,
                executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
                ids: &[EntityId]
            ) -> Result<std::collections::HashMap<EntityId, Out>, es_entity::EsRepoError> {
                #[derive(sqlx::FromRow)]
                struct EventRow {
                    id: EntityId,
                    sequence: i32,
                    event: es_entity::prelude::serde_json::Value,
                    recorded_at: es_entity::prelude::chrono::DateTime<es_entity::prelude::chrono::Utc>,
                }

                let rows: Vec<EventRow> = sqlx::query_as("SELECT i.id, e.sequence, e.event, e.recorded_at FROM entities i JOIN entity_events e ON i.id = e.id WHERE i.id = ANY($1) ORDER BY i.id, e.sequence")
                    .bind(ids)
                    .fetch_all(executor)
                    .await?;

                let n = rows.len();
                let res = es_entity::EntityEvents::load_n::<Entity>(rows.into_iter().map(|r|
                        es_entity::GenericEvent {
                            entity_id: r.id,
                            sequence: r.sequence,
                            event: r.event,
                            recorded_at: r.recorded_at,
                        }), n)?;

                Ok(res.0.into_iter().map(|u| (u.id.clone(), Out::from(u))).collect())
            }
        };

        assert_eq!(tokens.to_string(), expected.to_string());
    }
}
