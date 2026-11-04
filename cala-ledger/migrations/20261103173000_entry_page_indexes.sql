-- "Entries for account (set)" pagination (entry repo's
-- list_for_account_{set_}id_by_created_at) joins balance rows to entries by
-- latest_entry_id and orders pages by entry created_at. Together these
-- indexes let the planner walk entries newest-first (index-only, no sort)
-- probing the balance index per entry, so a page costs O(page) however much
-- balance history the account carries.
CREATE INDEX idx_cala_balance_history_account_id_latest_entry_id ON cala_balance_history (account_id, latest_entry_id);
CREATE INDEX idx_cala_entries_created_at_id ON cala_entries (created_at DESC, id DESC);
