import type { SidebarsConfig } from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  demoSidebar: [
    'intro',
    {
      type: 'category',
      label: 'GraphQL API Demo',
      collapsed: false,
      items: [
        'demo/create-journal-and-accounts',
        'demo/tx-template-create',
        'demo/transaction-post',
        'demo/check-account-balance',
        'demo/account-set',
      ]
    },
    {
      type: 'link',
      label: 'API Reference',
      href: 'https://cala.sh/api-reference.html',
    },
    {
      type: 'link',
      label: 'Rust Crate Docs',
      href: 'https://docs.rs/cala-ledger/latest/cala_ledger/',
    },
  ],
  accountingSidebar: [
    'accounting/accounting-intro',
    'accounting/glossary',
    'accounting/double-entry-accounting',
    'accounting/step-by-step',
  ],
};

export default sidebars;
