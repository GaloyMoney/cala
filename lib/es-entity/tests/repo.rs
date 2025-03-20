#![allow(dead_code)]
mod journal;

use es_entity::*;
use sqlx::PgPool;

use journal::*;

#[derive(EsRepo, Debug)]
#[es_repo(
    entity = "Journal",
    err = "JournalError",
    columns(data_source_id(
        ty = "JournalId",
        create(accessor = "data_source()"),
        update(persist = false)
    ),),
    tbl_prefix = "cala"
)]
struct JournalRepo<E> {
    pool: PgPool,
    _phantom: std::marker::PhantomData<E>,
}

#[tokio::test]
async fn test() {
    assert!(true);
}
