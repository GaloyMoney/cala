# "Chart of Accounts" Account Set
resource "cala_account_set" "chart_of_accounts" {
  id         = "00000000-0000-0000-0000-100000000001"
  journal_id = cala_journal.journal.id
  name       = "Chart of Accounts"
}

# EQUITY
resource "random_uuid" "equity" {}
resource "cala_account_set" "equity" {
  id                  = random_uuid.equity.result
  journal_id          = cala_journal.journal.id
  name                = "Equity"
  normal_balance_type = "CREDIT"
}
resource "cala_account_set_member_account_set" "equity" {
  account_set_id        = cala_account_set.chart_of_accounts.id
  member_account_set_id = cala_account_set.equity.id
}

resource "random_uuid" "retained_earnings" {}
resource "cala_account_set" "retained_earnings" {
  id                  = random_uuid.retained_earnings.result
  journal_id          = cala_journal.journal.id
  name                = "Retained Earnings"
  normal_balance_type = "CREDIT"
}
resource "cala_account_set_member_account_set" "retained_earnings_in_equity" {
  account_set_id        = cala_account_set.equity.id # this should be 'equity'
  member_account_set_id = cala_account_set.retained_earnings.id
}


# REVENUE
resource "random_uuid" "revenue" {}
resource "cala_account_set" "revenue" {
  id                  = random_uuid.revenue.result
  journal_id          = cala_journal.journal.id
  name                = "Revenue"
  normal_balance_type = "CREDIT"
}
resource "cala_account_set_member_account_set" "revenue" {
  account_set_id        = cala_account_set.chart_of_accounts.id
  member_account_set_id = cala_account_set.revenue.id
}
resource "cala_account_set_member_account_set" "revenue_in_retained_earnings" {
  account_set_id        = cala_account_set.retained_earnings.id
  member_account_set_id = cala_account_set.revenue.id
}

# REVENUE: Members
resource "cala_account_set" "interest_revenue_control" {
  id                  = "00000000-0000-0000-0000-140000000001"
  journal_id          = cala_journal.journal.id
  name                = "Interest Revenue Control Account"
  normal_balance_type = "CREDIT"
}
resource "cala_account_set_member_account_set" "interest_revenue_control_in_revenue" {
  account_set_id        = cala_account_set.revenue.id
  member_account_set_id = cala_account_set.interest_revenue_control.id
}

