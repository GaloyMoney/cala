//! Downstream-style compile contracts: adding an impossible case breaks these
//! exhaustive matches. These fixtures intentionally do not poll database I/O.
#![allow(dead_code)]
use cala_ledger::{
    account::{error::*, Account},
    account_set::error::*,
    errlanes::{lanes, Fail, Rejection},
    journal::{error::*, Journal},
    tx_template::error::*,
    velocity::error::*,
    *,
};
use std::future::Future;

fn io<T, R: Rejection>(_: impl Future<Output = Result<T, Fail<R, lanes!(Transient, Fatal)>>>) {}
fn public_signatures(cala: &CalaLedger, account: &mut Account, journal: &mut Journal) {
    io::<_, PersistAccountRejection>(cala.accounts().persist(account));
    io::<_, PersistJournalRejection>(cala.journals().persist(journal));
    io::<_, AccountNotFound>(cala.accounts().find(AccountId::new()));
    io::<_, AccountCodeNotFound>(cala.accounts().find_by_code("code".into()));
    io::<_, AccountExternalIdNotFound>(cala.accounts().find_by_external_id("external".into()));
    io::<_, JournalNotFound>(cala.journals().find(JournalId::new()));
    io::<_, JournalCodeNotFound>(cala.journals().find_by_code("code".into()));
    io::<_, TxTemplateNotFound>(cala.tx_templates().find_by_code("code"));
    io::<_, AddAccountMembersRejection>(cala.account_sets().add_members(&[]));
    io::<_, AddSetMembersRejection>(cala.account_sets().add_member_sets(&[]));
    io::<_, RemoveMemberRejection>(
        cala.account_sets()
            .remove_member(AccountSetId::new(), AccountId::new()),
    );
    io::<_, posting::PostingRejection>(cala.post_transaction(
        TransactionId::new(),
        "template",
        tx_template::Params::new(),
    ));
    io::<_, posting::BatchPostingRejection>(cala.post_transactions(vec![]));
}
fn template_create(e: CreateTxTemplateRejection) {
    match e {
        CreateTxTemplateRejection::DuplicateId(_) | CreateTxTemplateRejection::DuplicateCode(_) => {
        }
    }
}
fn account_persist(e: PersistAccountRejection) {
    match e {
        PersistAccountRejection::CodeAlreadyExists(_)
        | PersistAccountRejection::ExternalIdAlreadyExists(_)
        | PersistAccountRejection::CannotUpdateAccountSetAccounts => {}
    }
}
fn journal_persist(e: PersistJournalRejection) {
    match e {
        PersistJournalRejection::CodeAlreadyExists(_) => {}
    }
}
fn account_members(e: AddAccountMembersRejection) {
    match e {
        AddAccountMembersRejection::AccountSetNotFound(_)
        | AddAccountMembersRejection::MemberHasBalanceHistory(_)
        | AddAccountMembersRejection::MemberAlreadyAdded(_) => {}
    }
}
fn remove_members(e: RemoveMemberRejection) {
    match e {
        RemoveMemberRejection::AccountSetNotFound(_)
        | RemoveMemberRejection::MemberHasBalanceHistory(_)
        | RemoveMemberRejection::JournalIdMismatch => {}
    }
}
fn velocity_enforce(e: EnforceVelocityBatchRejection) {
    match e {
        EnforceVelocityBatchRejection::CoreTypeCoercion(_)
        | EnforceVelocityBatchRejection::UnknownIdent { .. }
        | EnforceVelocityBatchRejection::MissingArgument { .. }
        | EnforceVelocityBatchRejection::NoMatchingOverload { .. }
        | EnforceVelocityBatchRejection::Unexpected { .. }
        | EnforceVelocityBatchRejection::UnsupportedOpaque { .. }
        | EnforceVelocityBatchRejection::OpaqueDowncast { .. }
        | EnforceVelocityBatchRejection::FunctionValue { .. }
        | EnforceVelocityBatchRejection::NonStringKey(_)
        | EnforceVelocityBatchRejection::UnsupportedBytes { .. }
        | EnforceVelocityBatchRejection::LimitExceeded(_) => {}
    }
}
