import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Cala',
  tagline: 'a powerful open source core banking ledger',
  favicon: 'img/favicon.ico',

  // production url
  url: 'https://cala.sh/',
  // Set the /<baseUrl>/ pathname under which your site is served
  // For GitHub pages deployment, it is often '/<projectName>/'
  baseUrl: '/',

  // GitHub pages deployment config.
  // If you aren't using GitHub pages, you don't need these.
  organizationName: 'GaloyMoney', // Usually your GitHub org/user name.
  projectName: 'cala', // Usually your repo name.

  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: '/', // This changes the base path from /docs
          //editUrl: ({versionDocsDirPath, docPath}) => {
          //  return `https://github.com/GaloyMoney/cala/edit/main//website/${versionDocsDirPath}/${docPath}`;
          //},
          //showLastUpdateAuthor: false,
          //showLastUpdateTime: false,
        },
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    // social card
    image: 'img/galoy.png',
    navbar: {
      title: 'Cala',
      logo: {
        alt: 'Cala Logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'demoSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          type: 'docSidebar',
          sidebarId: 'accountingSidebar',
          position: 'left',
          label: 'Accounting Intro',
        },
        {
          href: 'https://cala.sh/api-reference.html',
          label: 'API Reference',
          position: 'left',
        },
        {
          href: 'https://docs.rs/cala-ledger/latest/cala_ledger/',
          label: 'Rust crate docs',
          position: 'left',
        },
        {
          href: 'https://github.com/GaloyMoney/cala',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {
              label: 'Try Cala',
              to: '/docs',
            },
            {
              label: 'GraphQL API demo',
              to: '/docs/create-journal-and-accounts',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'Mattermost',
              href: 'https://chat.galoy.io',
            },
            {
              label: 'X / Twitter',
              href: 'https://x.com/GaloyMoney',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/GaloyMoney',
            },
            {
              label: 'Docs.rs',
              href: 'https://docs.rs/cala-ledger/latest/cala_ledger',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Galoy Inc.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
    colorMode: {
      defaultMode: 'light',
      disableSwitch: false, //manual dark mode switch
      respectPrefersColorScheme: false, // system dark mode switch
    },
    liveCodeBlock: {
      /**
       * The position of the live playground, above or under the editor
       * Possible values: "top" | "bottom"
       */
      playgroundPosition: 'bottom',
    },
  } satisfies Preset.ThemeConfig,

  markdown: {
    mermaid: true,
  },
  themes: ['@docusaurus/theme-live-codeblock','@docusaurus/theme-mermaid'],
};

export default config;
