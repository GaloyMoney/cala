import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  demoSidebar: [
      'intro',
    {
      type: 'category',
      label: 'GraphQL API demo',
      collapsed: false,
      items: [
        'demo/create-journal-and-accounts',
        'demo/tx-template-create',
        'demo/post-transaction',
        'demo/check-account-balance',
      ]
    },
    {
      type: 'link',
      label: 'API reference',
      href: 'https://cala.sh/api-reference.html',
    },
    {
      type: 'link',
      label: 'Rust crate docs',
      href: 'https://docs.rs/cala-ledger/latest/cala_ledger/',
    },
  ],
};

export default sidebars;
