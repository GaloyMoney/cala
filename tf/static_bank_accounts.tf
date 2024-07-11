# Entrypoint
resource "cala_account_set" "chart_of_accounts" {
  id                  = "00000000-0000-0000-0000-100000000000"
  journal_id          = cala_journal.journal.id
  name                = "Chart of Accounts"
  normal_balance_type = "DEBIT"
}

# Account #1
resource "random_uuid" "bank_shareholder_equity" {}
resource "cala_account" "bank_shareholder_equity" {
  id                  = random_uuid.bank_shareholder_equity.result
  name                = "(Account #1) Bank Shareholder Equity"
  code                = "BANK.SHAREHOLDER_EQUITY"
  normal_balance_type = "CREDIT"
}
resource "cala_account_set_member_account" "bank_shareholder_equity" {
  account_set_id    = cala_account_set.chart_of_accounts.id
  member_account_id = cala_account.bank_shareholder_equity.id
}


# AccountSet #2
resource "random_uuid" "user_control" {}
resource "cala_account_set" "user_control" {
  id                  = random_uuid.user_control.result
  journal_id          = cala_journal.journal.id
  name                = "(AccountSet #2) User Control"
  normal_balance_type = "CREDIT"
  depends_on          = [cala_account.bank_shareholder_equity, cala_account_set_member_account.bank_shareholder_equity]
}
resource "cala_account_set_member_account_set" "user_control" {
  account_set_id        = cala_account_set.chart_of_accounts.id
  member_account_set_id = cala_account_set.user_control.id
}

# Account #3
resource "random_uuid" "bank_reserve" {}
resource "cala_account" "bank_reserve" {
  id                  = random_uuid.bank_reserve.result
  name                = "(Account #3) Bank Reserve from Shareholders"
  code                = "BANK.RESERVE_FROM_SHAREHOLDER"
  normal_balance_type = "DEBIT"
  depends_on          = [cala_account_set.user_control, cala_account_set_member_account_set.user_control]
}
resource "cala_account_set_member_account" "bank_reserve" {
  account_set_id    = cala_account_set.chart_of_accounts.id
  member_account_id = cala_account.bank_reserve.id
}

# Account #4
resource "random_uuid" "revenue" {}
resource "cala_account" "revenue" {
  id                  = random_uuid.revenue.result
  name                = "(Account #4) Revenue"
  code                = "BANK.REVENUE"
  normal_balance_type = "CREDIT"
  depends_on          = [cala_account.bank_reserve, cala_account_set_member_account.bank_reserve]
}
resource "cala_account_set_member_account" "revenue" {
  account_set_id    = cala_account_set.chart_of_accounts.id
  member_account_id = cala_account.revenue.id
}

# AccountSet #5
resource "random_uuid" "loans_control" {}
resource "cala_account_set" "loans_control" {
  id                  = random_uuid.loans_control.result
  journal_id          = cala_journal.journal.id
  name                = "(AccountSet #5) Loans Control"
  normal_balance_type = "DEBIT"
  depends_on          = [cala_account.revenue, cala_account_set_member_account.revenue]
}
resource "cala_account_set_member_account_set" "loans_control" {
  account_set_id        = cala_account_set.chart_of_accounts.id
  member_account_set_id = cala_account_set.loans_control.id
}

# AccountSet #6
resource "random_uuid" "deposits_control" {}
resource "cala_account_set" "deposits_control" {
  id                  = random_uuid.deposits_control.result
  journal_id          = cala_journal.journal.id
  name                = "(AccountSet #6) Deposits Control"
  normal_balance_type = "DEBIT"
  depends_on          = [cala_account_set.loans_control, cala_account_set_member_account_set.loans_control]
}
resource "cala_account_set_member_account_set" "deposits_control" {
  account_set_id        = cala_account_set.chart_of_accounts.id
  member_account_set_id = cala_account_set.deposits_control.id
}

# AccountSet #7
resource "random_uuid" "withdrawals_control" {}
resource "cala_account_set" "withdrawals_control" {
  id                  = random_uuid.withdrawals_control.result
  journal_id          = cala_journal.journal.id
  name                = "(AccountSet #7) Withdrawals Control"
  normal_balance_type = "DEBIT"
  depends_on          = [cala_account_set.deposits_control, cala_account_set_member_account_set.deposits_control]
}
resource "cala_account_set_member_account_set" "withdrawals_control" {
  account_set_id        = cala_account_set.chart_of_accounts.id
  member_account_set_id = cala_account_set.withdrawals_control.id
}

# Account #8
resource "random_uuid" "expenses" {}
resource "cala_account" "expenses" {
  id                  = random_uuid.expenses.result
  name                = "(Account #8) Expenses"
  code                = "BANK.Expenses"
  normal_balance_type = "DEBIT"
  depends_on          = [cala_account_set.withdrawals_control, cala_account_set_member_account_set.withdrawals_control]
}
resource "cala_account_set_member_account" "expenses" {
  account_set_id    = cala_account_set.chart_of_accounts.id
  member_account_id = cala_account.expenses.id
}
