import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  demoSidebar: [
      'intro',
      'demo/start',
    {
      type: 'category',
      label: 'GraphQL API demo',
      collapsed: false,
      items: [
        'demo/journal-create',
        'demo/account-create',
        'demo/tx-template-create'
      ]
    },
  ],
};

export default sidebars;
