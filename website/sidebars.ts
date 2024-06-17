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
      label: 'API Reference',
      href: 'https://cala.sh/api-reference.html',
    },
  ],
};

export default sidebars;
