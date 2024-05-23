import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  demoSidebar: [
      'intro',
    {
      type: 'category',
      label: 'Demo',
      collapsed: false,
      items: [
        'demo/journalcreate'
      ]
    },
  ],
};

export default sidebars;
