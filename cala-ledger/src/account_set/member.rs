use crate::primitives::*;

pub enum AccountSetMember {
    Account(AccountId),
    // AccountSet(AccountSetId),
}

impl From<AccountId> for AccountSetMember {
    fn from(account_id: AccountId) -> Self {
        AccountSetMember::Account(account_id)
    }
}
