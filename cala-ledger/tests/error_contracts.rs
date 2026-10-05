//! Downstream-style compile contracts: adding an impossible case breaks these
//! exhaustive matches. These fixtures intentionally do not poll database I/O.
#![allow(dead_code)]
use cala_ledger::{
    account::{error::*, Account},
    account_set::error::*,
    errlanes::{lanes, Fail, Rejection},
    journal::{error::*, Journal},
    tx_template::error::*,
    velocity::error::AttachVelocityControlRejection,
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
    io::<_, AttachVelocityControlRejection>(cala.velocities().attach_control_to_account(
        VelocityControlId::new(),
        AccountId::new(),
        tx_template::Params::new(),
    ));
}

fn velocity_attachment(e: AttachVelocityControlRejection) {
    match e {
        AttachVelocityControlRejection::ControlNotFound(_)
        | AttachVelocityControlRejection::Param(_)
        | AttachVelocityControlRejection::Default(_)
        | AttachVelocityControlRejection::Cel(_) => {}
    }
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

fn mixed_members(e: AddMemberRejection) {
    match e {
        AddMemberRejection::AccountSetNotFound(_)
        | AddMemberRejection::MemberHasBalanceHistory(_)
        | AddMemberRejection::MemberAlreadyAdded(_)
        | AddMemberRejection::JournalIdMismatch
        | AddMemberRejection::MembershipCycleDetected { .. }
        | AddMemberRejection::MembershipDepthExceeded { .. } => {}
    }
}

fn account_status(e: SetAccountStatusRejection) {
    match e {
        SetAccountStatusRejection::AccountNotFound(_)
        | SetAccountStatusRejection::CodeAlreadyExists(_)
        | SetAccountStatusRejection::ExternalIdAlreadyExists(_)
        | SetAccountStatusRejection::CannotUpdateAccountSetAccounts => {}
    }
}

#[test]
fn membership_union_preserves_shared_leaf_identity_and_diagnostics() {
    use std::error::Error;

    let account_set_id = AccountSetId::new();
    let member_id = AccountId::new();
    let history = || MemberHasBalanceHistory {
        account_set_id,
        member_id,
    };
    let from_accounts = [
        AddMemberRejection::from(AddAccountMembersRejection::AccountSetNotFound(
            AccountSetNotFound(account_set_id),
        )),
        AddMemberRejection::from(AddAccountMembersRejection::MemberHasBalanceHistory(
            history(),
        )),
        AddMemberRejection::from(AddAccountMembersRejection::MemberAlreadyAdded(
            MemberAlreadyAdded,
        )),
    ];
    let from_sets = [
        AddMemberRejection::from(AddSetMembersRejection::AccountSetNotFound(
            AccountSetNotFound(account_set_id),
        )),
        AddMemberRejection::from(AddSetMembersRejection::MemberHasBalanceHistory(history())),
        AddMemberRejection::from(AddSetMembersRejection::MemberAlreadyAdded(
            MemberAlreadyAdded,
        )),
    ];
    for (account, set) in from_accounts.into_iter().zip(from_sets) {
        assert_eq!(<&str>::from(account.code()), <&str>::from(set.code()));
        assert_eq!(account.level(), set.level());
        assert_eq!(account.to_string(), set.to_string());
        for rejection in [account, set] {
            match &rejection {
                AddMemberRejection::AccountSetNotFound(AccountSetNotFound(id)) => {
                    assert_eq!(*id, account_set_id);
                    assert!(rejection.source().unwrap().is::<AccountSetNotFound>());
                }
                AddMemberRejection::MemberHasBalanceHistory(detail) => {
                    assert_eq!(detail.account_set_id, account_set_id);
                    assert_eq!(detail.member_id, member_id);
                    assert!(rejection.source().unwrap().is::<MemberHasBalanceHistory>());
                }
                AddMemberRejection::MemberAlreadyAdded(_) => {
                    assert!(rejection.source().unwrap().is::<MemberAlreadyAdded>());
                }
                other => panic!("unexpected shared membership outcome: {other:?}"),
            }
        }
    }
}

#[test]
fn membership_union_inherits_the_public_graph_diagnostics() {
    let account_set_id = AccountSetId::new();
    let member_account_set_id = AccountSetId::new();
    for (source, code) in [
        (
            AddSetMembersRejection::JournalIdMismatch,
            "CALA_ACCOUNT_SET_JOURNAL_ID_MISMATCH",
        ),
        (
            AddSetMembersRejection::MembershipCycleDetected {
                account_set_id,
                member_account_set_id,
            },
            "CALA_ACCOUNT_SET_MEMBERSHIP_CYCLE_DETECTED",
        ),
        (
            AddSetMembersRejection::MembershipDepthExceeded {
                account_set_id,
                member_account_set_id,
                depth: 9,
                max: 8,
            },
            "CALA_ACCOUNT_SET_MEMBERSHIP_DEPTH_EXCEEDED",
        ),
    ] {
        assert_eq!(<&str>::from(source.code()), code);
        let display = source.to_string();
        let level = source.level();
        let destination = AddMemberRejection::from(source);
        assert_eq!(<&str>::from(destination.code()), code);
        assert_eq!(destination.to_string(), display);
        assert_eq!(destination.level(), level);
        mixed_members(destination);
    }
}

#[test]
fn account_status_inherits_persistence_diagnostics() {
    for source in [
        PersistAccountRejection::CodeAlreadyExists(Some("existing".into())),
        PersistAccountRejection::ExternalIdAlreadyExists(Some(Some("external".into()))),
        PersistAccountRejection::CannotUpdateAccountSetAccounts,
    ] {
        let code = <&str>::from(source.code());
        let display = source.to_string();
        let level = source.level();
        let destination = SetAccountStatusRejection::from(source);
        assert_eq!(<&str>::from(destination.code()), code);
        assert_eq!(destination.to_string(), display);
        assert_eq!(destination.level(), level);
        account_status(destination);
    }
}
