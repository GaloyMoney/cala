---
id: glossary
title: Glossary of Terms
slug: /accounting/glossary
---

## Account
An account is a record in the general ledger that tracks financial transactions. Accounts are categorized as assets, liabilities, equity, revenues, or expenses. In Cala, each account has a unique identifier (`accountId`).

## Journal
A journal is a chronological record of all financial transactions. Each journal entry includes the date, accounts affected, and the amounts debited or credited. In Cala, each journal has a unique identifier (`journalId`).

## Transaction
A transaction is a financial event that affects the accounts in the general ledger. Transactions are recorded in journals and typically involve debits and credits to different accounts.

## Debit and Credit
These are the two sides of every financial transaction:
- **Debit**: An entry on the left side of an account ledger that increases asset or expense accounts and decreases liability, equity, or revenue accounts.
- **Credit**: An entry on the right side of an account ledger that increases liability, equity, or revenue accounts and decreases asset or expense accounts.

## Balance
The balance of an account is the difference between the total debits and total credits recorded in that account. It indicates the current amount available or owed.

## Currency
Currency represents the type of money being used in transactions, such as USD (United States Dollar).

## Transaction Template
A transaction template is a predefined structure for a specific type of transaction, such as deposits or withdrawals. It ensures consistency and accuracy in recurring transactions by specifying the required parameters and transaction details.

## UUID (Universally Unique Identifier)
A UUID is a 128-bit number used to uniquely identify information in computer systems. In Cala, it is used to uniquely identify accounts, journals, and transactions.

## Account Set
An Account Set is a grouping of accounts within a financial system, designed to facilitate the management and reporting of related financial activities. Each Account Set is associated with a specific journal and consists of multiple accounts, each contributing to the collective balance of the set. Account Sets allow for the application of transaction templates and the tracking of consolidated balances, aiding in detailed financial analysis and reporting. In Cala, each Account Set has a unique identifier (`accountSetId`).