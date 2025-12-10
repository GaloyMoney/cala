use async_graphql::{dataloader::*, types::connection::*, *};
use cala_ledger::{balance::AccountBalance, primitives::*, tx_template::NewParamDefinition};

use crate::{app::CalaApp, extension::*};

use super::{
    account::*, account_set::*, balance::*, journal::*, loader::*, primitives::*, transaction::*,
    tx_template::*, velocity::*,
};

#[derive(Default)]
pub struct CoreQuery<E: QueryExtensionMarker> {
    _phantom: std::marker::PhantomData<E>,
}

#[Object(name = "Query")]
impl<E: QueryExtensionMarker> CoreQuery<E> {
    #[graphql(flatten)]
    async fn extension(&self) -> E {
        E::default()
    }

    async fn account(&self, ctx: &Context<'_>, id: UUID) -> async_graphql::Result<Option<Account>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(AccountId::from(id)).await?)
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
            Ok(account) => Ok(Some(account.into())),
            Err(cala_ledger::account::error::AccountError::CouldNotFindByExternalId(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn account_by_code(
        &self,
        ctx: &Context<'_>,
        code: String,
    ) -> async_graphql::Result<Option<Account>> {
        let app = ctx.data_unchecked::<CalaApp>();
        match app.ledger().accounts().find_by_code(code).await {
            Ok(account) => Ok(Some(account.into())),
            Err(cala_ledger::account::error::AccountError::CouldNotFindByCode(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn accounts(
        &self,
        ctx: &Context<'_>,
        first: i32,
        after: Option<String>,
    ) -> Result<Connection<AccountsByNameCursor, Account, EmptyFields, EmptyFields>> {
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
                    .list(cala_ledger::es_entity::PaginatedQueryArgs { first, after })
                    .await?;
                let mut connection = Connection::new(false, result.has_next_page);
                connection
                    .edges
                    .extend(result.entities.into_iter().map(|entity| {
                        let cursor = AccountsByNameCursor::from(&entity);
                        Edge::new(cursor, Account::from(entity))
                    }));
                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }

    async fn account_set(
        &self,
        ctx: &Context<'_>,
        id: UUID,
    ) -> async_graphql::Result<Option<AccountSet>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(AccountSetId::from(id)).await?)
    }

    async fn journal(&self, ctx: &Context<'_>, id: UUID) -> async_graphql::Result<Option<Journal>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(JournalId::from(id)).await?)
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

    async fn transaction(
        &self,
        ctx: &Context<'_>,
        id: UUID,
    ) -> async_graphql::Result<Option<Transaction>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(TransactionId::from(id)).await?)
    }

    async fn transaction_by_external_id(
        &self,
        ctx: &Context<'_>,
        external_id: String,
    ) -> async_graphql::Result<Option<Transaction>> {
        let app = ctx.data_unchecked::<CalaApp>();
        match app
            .ledger()
            .transactions()
            .find_by_external_id(external_id)
            .await
        {
            Ok(transaction) => Ok(Some(transaction.into())),
            Err(cala_ledger::transaction::error::TransactionError::CouldNotFindByExternalId(_)) => {
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn tx_template(
        &self,
        ctx: &Context<'_>,
        id: UUID,
    ) -> async_graphql::Result<Option<TxTemplate>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(TxTemplateId::from(id)).await?)
    }

    async fn tx_template_by_code(
        &self,
        ctx: &Context<'_>,
        code: String,
    ) -> async_graphql::Result<Option<TxTemplate>> {
        let app = ctx.data_unchecked::<CalaApp>();
        match app.ledger().tx_templates().find_by_code(code).await {
            Ok(tx_template) => Ok(Some(tx_template.into())),
            Err(cala_ledger::tx_template::error::TxTemplateError::CouldNotFindByCode(_)) => {
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn velocity_limit(
        &self,
        ctx: &Context<'_>,
        id: UUID,
    ) -> async_graphql::Result<Option<VelocityLimit>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(VelocityLimitId::from(id)).await?)
    }

    async fn velocity_control(
        &self,
        ctx: &Context<'_>,
        id: UUID,
    ) -> async_graphql::Result<Option<VelocityControl>> {
        let loader = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        Ok(loader.load_one(VelocityControlId::from(id)).await?)
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
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let mut builder = cala_ledger::account::NewAccount::builder();
        builder
            .id(input.account_id)
            .name(input.name)
            .code(input.code)
            .normal_balance_type(input.normal_balance_type)
            .status(input.status);

        if let Some(external_id) = input.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }
        if let Some(metadata) = input.metadata {
            builder.metadata(metadata)?;
        }
        let account = app
            .ledger()
            .accounts()
            .create_in_op(&mut *op, builder.build()?)
            .await?;

        if let Some(account_set_ids) = input.account_set_ids {
            for id in account_set_ids {
                app.ledger()
                    .account_sets()
                    .add_member_in_op(&mut *op, AccountSetId::from(id), account.id())
                    .await?;
            }
        }

        Ok(account.into())
    }

    async fn account_update(
        &self,
        ctx: &Context<'_>,
        id: UUID,
        input: AccountUpdateInput,
    ) -> Result<AccountUpdatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let mut builder = cala_ledger::account::AccountUpdate::default();
        if let Some(name) = input.name {
            builder.name(name);
        }
        if let Some(code) = input.code {
            builder.code(code);
        }
        if let Some(normal_balance_type) = input.normal_balance_type {
            builder.normal_balance_type(normal_balance_type);
        }
        if let Some(status) = input.status {
            builder.status(status);
        }
        if let Some(external_id) = input.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }
        if let Some(metadata) = input.metadata {
            builder.metadata(metadata)?;
        }

        let mut account = app.ledger().accounts().find(AccountId::from(id)).await?;
        if account.update(builder).did_execute() {
            app.ledger()
                .accounts()
                .persist_in_op(&mut *op, &mut account)
                .await?;
        }

        Ok(account.into())
    }

    async fn account_set_create(
        &self,
        ctx: &Context<'_>,
        input: AccountSetCreateInput,
    ) -> Result<AccountSetCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let mut builder = cala_ledger::account_set::NewAccountSet::builder();
        builder
            .id(input.account_set_id)
            .journal_id(input.journal_id)
            .name(input.name)
            .normal_balance_type(input.normal_balance_type);

        if let Some(description) = input.description {
            builder.description(description);
        }
        if let Some(metadata) = input.metadata {
            builder.metadata(metadata)?;
        }
        let account_set = app
            .ledger()
            .account_sets()
            .create_in_op(&mut *op, builder.build()?)
            .await?;

        Ok(account_set.into())
    }

    async fn account_set_update(
        &self,
        ctx: &Context<'_>,
        id: UUID,
        input: AccountSetUpdateInput,
    ) -> Result<AccountSetUpdatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let mut builder = cala_ledger::account_set::AccountSetUpdate::default();
        if let Some(name) = input.name {
            builder.name(name);
        }
        if let Some(desc) = input.description {
            builder.description(desc);
        }
        if let Some(normal_balance_type) = input.normal_balance_type {
            builder.normal_balance_type(normal_balance_type);
        }

        if let Some(metadata) = input.metadata {
            builder.metadata(metadata)?;
        }

        let mut account_set = app
            .ledger()
            .account_sets()
            .find(AccountSetId::from(id))
            .await?;
        if account_set.update(builder).did_execute() {
            app.ledger()
                .account_sets()
                .persist_in_op(&mut *op, &mut account_set)
                .await?;
        }

        Ok(account_set.into())
    }

    async fn add_to_account_set(
        &self,
        ctx: &Context<'_>,
        input: AddToAccountSetInput,
    ) -> Result<AddToAccountSetPayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let account_set = app
            .ledger()
            .account_sets()
            .add_member_in_op(&mut *op, AccountSetId::from(input.account_set_id), input)
            .await?;

        Ok(account_set.into())
    }

    async fn remove_from_account_set(
        &self,
        ctx: &Context<'_>,
        input: RemoveFromAccountSetInput,
    ) -> Result<RemoveFromAccountSetPayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let account_set = app
            .ledger()
            .account_sets()
            .remove_member_in_op(&mut *op, AccountSetId::from(input.account_set_id), input)
            .await?;

        Ok(account_set.into())
    }

    async fn journal_create(
        &self,
        ctx: &Context<'_>,
        input: JournalCreateInput,
    ) -> Result<JournalCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let mut builder = cala_ledger::journal::NewJournal::builder();
        builder
            .id(input.journal_id)
            .name(input.name)
            .status(input.status);
        if let Some(description) = input.description {
            builder.description(description);
        }
        let journal = app
            .ledger()
            .journals()
            .create_in_op(&mut *op, builder.build()?)
            .await?;

        Ok(journal.into())
    }

    async fn journal_update(
        &self,
        ctx: &Context<'_>,
        id: UUID,
        input: JournalUpdateInput,
    ) -> Result<JournalUpdatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let mut builder = cala_ledger::journal::JournalUpdate::default();
        if let Some(name) = input.name {
            builder.name(name);
        }
        if let Some(status) = input.status {
            builder.status(status);
        }
        if let Some(description) = input.description {
            builder.description(description);
        }

        let mut journal = app.ledger().journals().find(JournalId::from(id)).await?;
        if journal.update(builder).did_execute() {
            app.ledger()
                .journals()
                .persist_in_op(&mut *op, &mut journal)
                .await?;
        }

        Ok(journal.into())
    }

    async fn tx_template_create(
        &self,
        ctx: &Context<'_>,
        input: TxTemplateCreateInput,
    ) -> Result<TxTemplateCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let mut new_tx_template_transaction_builder =
            cala_ledger::tx_template::NewTxTemplateTransaction::builder();
        let TxTemplateTransactionInput {
            effective,
            journal_id,
            correlation_id,
            external_id,
            description,
            metadata,
        } = input.transaction;
        new_tx_template_transaction_builder
            .effective(effective)
            .journal_id(journal_id);
        if let Some(correlation_id) = correlation_id {
            new_tx_template_transaction_builder.correlation_id(correlation_id);
        };
        if let Some(external_id) = external_id {
            new_tx_template_transaction_builder.external_id(external_id);
        };
        if let Some(description) = description {
            new_tx_template_transaction_builder.description(description);
        };
        if let Some(metadata) = metadata {
            new_tx_template_transaction_builder.metadata(metadata);
        }
        let new_transaction = new_tx_template_transaction_builder.build()?;

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
            let mut new_entry_input_builder =
                cala_ledger::tx_template::NewTxTemplateEntry::builder();
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
            .transaction(new_transaction)
            .params(new_params)
            .entries(new_entries);
        if let Some(desc) = input.description {
            new_tx_template_builder.description(desc);
        }
        if let Some(metadata) = input.metadata {
            new_tx_template_builder.metadata(metadata)?;
        }
        let new_tx_template = new_tx_template_builder.build()?;

        let tx_template = app
            .ledger()
            .tx_templates()
            .create_in_op(&mut *op, new_tx_template)
            .await?;

        Ok(tx_template.into())
    }

    async fn transaction_post(
        &self,
        ctx: &Context<'_>,
        input: TransactionInput,
    ) -> Result<TransactionPostPayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let params = input.params.map(cala_ledger::tx_template::Params::from);
        let transaction = app
            .ledger()
            .post_transaction_in_op(
                &mut *op,
                input.transaction_id.into(),
                &input.tx_template_code,
                params.unwrap_or_default(),
            )
            .await?;
        Ok(transaction.into())
    }

    async fn velocity_limit_create(
        &self,
        ctx: &Context<'_>,
        input: VelocityLimitCreateInput,
    ) -> Result<VelocityLimitCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let mut new_velocity_limit_builder = cala_ledger::velocity::NewVelocityLimit::builder();
        new_velocity_limit_builder
            .id(input.velocity_limit_id)
            .name(input.name)
            .description(input.description);

        if let Some(condition) = input.condition {
            new_velocity_limit_builder.condition(condition);
        }

        if let Some(currency) = input.currency {
            new_velocity_limit_builder.currency(currency);
        }

        let mut new_window = Vec::new();
        for partition_key_input in input.window {
            let mut new_partition_key_builder = cala_ledger::velocity::NewPartitionKey::builder();
            let partition_key = new_partition_key_builder
                .alias(partition_key_input.alias)
                .value(partition_key_input.value)
                .build()?;

            new_window.push(partition_key);
        }
        new_velocity_limit_builder.window(new_window);

        let mut new_limit_builder = cala_ledger::velocity::NewLimit::builder();

        if let Some(timestamp_source) = input.limit.timestamp_source {
            new_limit_builder.timestamp_source(timestamp_source);
        }

        let mut new_balance_limits = Vec::new();
        for balance_limit_input in input.limit.balance {
            let mut new_balance_limit_builder = cala_ledger::velocity::NewBalanceLimit::builder();
            new_balance_limit_builder
                .limit_type(balance_limit_input.limit_type)
                .layer(balance_limit_input.layer)
                .amount(balance_limit_input.amount)
                .enforcement_direction(balance_limit_input.normal_balance_type);

            if let Some(start) = balance_limit_input.start {
                new_balance_limit_builder.start(start);
            }

            if let Some(end) = balance_limit_input.end {
                new_balance_limit_builder.end(end);
            }

            let new_balance_limit = new_balance_limit_builder.build()?;
            new_balance_limits.push(new_balance_limit);
        }
        new_limit_builder.balance(new_balance_limits);
        let new_limit = new_limit_builder.build()?;

        new_velocity_limit_builder.limit(new_limit);

        if let Some(params) = input.params {
            let mut new_params = Vec::new();
            for param in params {
                let mut param_builder = NewParamDefinition::builder();
                param_builder.name(param.name).r#type(param.r#type.into());
                if let Some(default) = param.default {
                    param_builder.default_expr(default);
                }
                if let Some(description) = param.description {
                    param_builder.description(description);
                }
                let new_param = param_builder.build()?;
                new_params.push(new_param);
            }
            new_velocity_limit_builder.params(new_params);
        }

        let new_velocity_limit = new_velocity_limit_builder.build()?;

        let velocity_limit = app
            .ledger()
            .velocities()
            .create_limit_in_op(&mut op, new_velocity_limit)
            .await?;

        Ok(velocity_limit.into())
    }

    async fn velocity_control_create(
        &self,
        ctx: &Context<'_>,
        input: VelocityControlCreateInput,
    ) -> Result<VelocityControlCreatePayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let mut new_velocity_control_builder = cala_ledger::velocity::NewVelocityControl::builder();
        new_velocity_control_builder
            .id(input.velocity_control_id)
            .name(input.name)
            .description(input.description);

        if let Some(condition) = input.condition {
            new_velocity_control_builder.condition(condition);
        }

        let mut new_velocity_enforcement_builder =
            cala_ledger::velocity::NewVelocityEnforcement::builder();
        new_velocity_enforcement_builder.action(input.enforcement.velocity_enforcement_action);
        let new_velocity_enforcement = new_velocity_enforcement_builder.build()?;

        new_velocity_control_builder.enforcement(new_velocity_enforcement);

        let new_velocity_control = new_velocity_control_builder.build()?;
        let velocity_control = app
            .ledger()
            .velocities()
            .create_control_in_op(&mut op, new_velocity_control)
            .await?;

        Ok(velocity_control.into())
    }

    async fn velocity_control_add_limit(
        &self,
        ctx: &Context<'_>,
        input: VelocityControlAddLimitInput,
    ) -> Result<VelocityControlAddLimitPayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");

        let velocity_limit = app
            .ledger()
            .velocities()
            .add_limit_to_control_in_op(
                &mut op,
                input.velocity_control_id.into(),
                input.velocity_limit_id.into(),
            )
            .await?;

        Ok(velocity_limit.into())
    }

    async fn velocity_control_attach(
        &self,
        ctx: &Context<'_>,
        input: VelocityControlAttachInput,
    ) -> Result<VelocityControlAttachPayload> {
        let app = ctx.data_unchecked::<CalaApp>();
        let mut op = ctx
            .data_unchecked::<DbOp>()
            .try_lock()
            .expect("Lock held concurrently");
        let params = cala_ledger::tx_template::Params::from(input.params);

        let velocity_control = app
            .ledger()
            .velocities()
            .attach_control_to_account_in_op(
                &mut op,
                input.velocity_control_id.into(),
                input.account_id.into(),
                params,
            )
            .await?;

        Ok(velocity_control.into())
    }
}
