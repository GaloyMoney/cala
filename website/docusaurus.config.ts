import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Cala',
  tagline: 'Cala Documentation',
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
          editUrl: ({versionDocsDirPath, docPath}) => {
            return `https://github.com/GaloyMoney/cala/edit/main/${versionDocsDirPath}/${docPath}`;
          },
          showLastUpdateAuthor: false,
          showLastUpdateTime: false,
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
      title: 'Cala documentation',
      logo: {
        alt: 'Cala Logo',
        src: 'img/logo.svg',
      },
      items: [
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
            //{
            //  label: 'Demo',
            //  to: '/docs/intro',
            //},
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
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Galoy Inc.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
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
