use async_graphql::{dataloader::*, types::connection::*, *};
use cala_ledger::{
    account::AccountValues, balance::AccountBalance, journal::JournalValues, primitives::*,
    tx_template::NewParamDefinition, tx_template::TxTemplateValues,
};

use super::{
    account::*, balance::*, job::*, journal::*, loader::*, primitives::*, transaction::*,
    tx_template::*,
};
use crate::{app::CalaApp, extension::MutationExtensionMarker};

pub struct Query;

#[Object]
impl Query {
    async fn account(&self, ctx: &Context<'_>, id: UUID) -> async_graphql::Result<Option<Account>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let account: Option<AccountValues> = loader.load_one(AccountId::from(id)).await?;
        Ok(account.map(Account::from))
    }

    async fn account_by_external_id(
        &self,
        ctx: &Context<'_>,
        external_id: String,
    ) -> async_graphql::Result<Option<Account>> {
        let app = ctx.data_unchecked::<CalaApp>();
        match app
            .ledger()
            .accounts()
            .find_by_external_id(external_id)
            .await
        {
            Ok(account) => Ok(Some(account.into_values().into())),
            Err(cala_ledger::account::error::AccountError::CouldNotFindByExternalId(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn accounts(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> Result<Connection<AccountByNameCursor, Account, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        query(
            after,
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let result = app
                    .ledger()
                    .accounts()
                    .list(cala_ledger::query::PaginatedQueryArgs {
                        first,
                        after: after.map(cala_ledger::account::AccountByNameCursor::from),
                    })
                    .await?;
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = AccountByNameCursor::from(entity.values());
                        Edge::new(cursor, Account::from(entity.into_values()))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }

    async fn journal(&self, ctx: &Context<'_>, id: UUID) -> async_graphql::Result<Option<Journal>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let journal: Option<JournalValues> = loader.load_one(JournalId::from(id)).await?;
        Ok(journal.map(Journal::from))
    }

    async fn balance(
        &self,
        ctx: &Context<'_>,
        journal_id: UUID,
        account_id: UUID,
        currency: CurrencyCode,
    ) -> async_graphql::Result<Option<Balance>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let journal_id = JournalId::from(journal_id);
        let account_id = AccountId::from(account_id);
        let currency = Currency::from(currency);
        let balance: Option<AccountBalance> =
            loader.load_one((journal_id, account_id, currency)).await?;
        Ok(balance.map(Balance::from))
    }

    async fn tx_template(
        &self,
        ctx: &Context<'_>,
        id: UUID,
    ) -> async_graphql::Result<Option<TxTemplate>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let tx_template: Option<TxTemplateValues> = loader.load_one(TxTemplateId::from(id)).await?;
        Ok(tx_template.map(TxTemplate::from))
    }

    async fn jobs(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> async_graphql::Result<Connection<JobByNameCursor, Job, EmptyFields, EmptyFields>> {
        let app = ctx.data_unchecked::<CalaApp>();
        query(
            after,
            None,
            Some(first),
            None,
            |after, _, first, _| async move {
                let first = first.expect("First always exists");
                let result = app
                    .list_jobs(cala_ledger::query::PaginatedQueryArgs {
                        first,
                        after: after.map(crate::job::JobByNameCursor::from),
                    })
                    .await?;
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = JobByNameCursor::from(&entity);
                        Edge::new(cursor, Job::from(entity))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }
}

#[derive(Default)]
pub struct CoreMutation<E: MutationExtensionMarker> {
    _phantom: std::marker::PhantomData<E>,
}

#[Object(name = "Mutation")]
impl<E: MutationExtensionMarker> CoreMutation<E> {
    #[graphql(flatten)]
    async fn extension(&self) -> E {
        E::default()
    }

    async fn account_create(
        &self,
        ctx: &Context<'_>,
        input: AccountCreateInput,
    ) -> Result<AccountCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut builder = cala_ledger::account::NewAccount::builder();
        builder
            .id(input.account_id)
            .name(input.name)
            .code(input.code)
            .normal_balance_type(input.normal_balance_type.into())
            .status(input.status.into());

        if let Some(external_id) = input.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }
        if let Some(metadata) = input.metadata {
            builder.metadata(metadata)?;
        }
        let account = app.ledger().accounts().create(builder.build()?).await?;

        Ok(account.into_values().into())
    }

    async fn journal_create(
        &self,
        ctx: &Context<'_>,
        input: JournalCreateInput,
    ) -> Result<JournalCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut builder = cala_ledger::journal::NewJournal::builder();
        builder.id(input.journal_id).name(input.name);
        if let Some(external_id) = input.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }
        let journal = app.ledger().journals().create(builder.build()?).await?;

        Ok(journal.into_values().into())
    }

    async fn tx_template_create(
        &self,
        ctx: &Context<'_>,
        input: TxTemplateCreateInput,
    ) -> Result<TxTemplateCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut new_tx_input_builder = cala_ledger::tx_template::NewTxInput::builder();
        let TxTemplateTxInput {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        } = input.tx_input;
        new_tx_input_builder
            .effective(effective)
            .journal_id(journal_id);
        if let Some(correlation_id) = correlation_id {
            new_tx_input_builder.correlation_id(correlation_id);
        };
        if let Some(external_id) = external_id {
            new_tx_input_builder.external_id(external_id);
        };
        if let Some(description) = description {
            new_tx_input_builder.description(description);
        };
        if let Some(metadata) = metadata {
            new_tx_input_builder.metadata(metadata);
        }
        let new_tx_input = new_tx_input_builder.build()?;

        let mut new_params = Vec::new();
        if let Some(params) = input.params {
            for param in params {
                let mut param_builder = NewParamDefinition::builder();
                param_builder.name(param.name).r#type(param.r#type.into());
                if let Some(default) = param.default {
                    param_builder.default_expr(default);
                }
                if let Some(desc) = param.description {
                    param_builder.description(desc);
                }
                let new_param = param_builder.build()?;
                new_params.push(new_param);
            }
        }

        let mut new_entries = Vec::new();
        for entry in input.entries {
            let TxTemplateEntryInput {
                entry_type,
                account_id,
                layer,
                direction,
                units,
                currency,
                description,
            } = entry;
            let mut new_entry_input_builder = cala_ledger::tx_template::NewEntryInput::builder();
            new_entry_input_builder
                .entry_type(entry_type)
                .account_id(account_id)
                .layer(layer)
                .direction(direction)
                .units(units)
                .currency(currency);
            if let Some(desc) = description {
                new_entry_input_builder.description(desc);
            }
            let new_entry_input = new_entry_input_builder.build()?;
            new_entries.push(new_entry_input);
        }

        let mut new_tx_template_builder = cala_ledger::tx_template::NewTxTemplate::builder();
        new_tx_template_builder
            .id(input.tx_template_id)
            .code(input.code)
            .tx_input(new_tx_input)
            .params(new_params)
            .entries(new_entries);
        if let Some(desc) = input.description {
            new_tx_template_builder.description(desc);
        }
        if let Some(metadata) = input.metadata {
            new_tx_template_builder.metadata(metadata)?;
        }
        let new_tx_template = new_tx_template_builder.build()?;
        let tx_template = app.ledger().tx_templates().create(new_tx_template).await?;

        Ok(tx_template.into_values().into())
    }

    async fn post_transaction(
        &self,
        ctx: &Context<'_>,
        input: TransactionInput,
    ) -> Result<PostTransactionPayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let params = input.params.map(cala_ledger::tx_template::TxParams::from);
        let transaction = app
            .ledger()
            .post_transaction(input.transaction_id.into(), &input.tx_template_code, params)
            .await?;
        Ok(transaction.into_values().into())
    }
}
