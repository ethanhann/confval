import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'confval',
  tagline: 'Span-first configuration parsing, validation, and lowering for Rust',
  favicon: 'img/favicon.ico',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  url: 'https://ethanhann.com',
  baseUrl: '/confval/',

  organizationName: 'ethanhann',
  projectName: 'confval',

  onBrokenLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  plugins: [
    [
      '@docusaurus/plugin-content-blog',
      {
        id: 'releases',
        routeBasePath: 'releases',
        path: './releases',
        blogTitle: 'Release',
        blogDescription: 'Confval release notes and changelogs.',
        blogSidebarTitle: 'All Releases',
        blogSidebarCount: 'ALL',
        showReadingTime: false,
        onUntruncatedBlogPosts: 'ignore',
        onInlineTags: 'warn',
        onInlineAuthors: 'warn',
        feedOptions: {
          type: ['rss', 'atom'],
          xslt: true,
        },
      },
    ],
  ],

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl:
            'https://github.com/ethanhann/confval/tree/main/docs/',
        },
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/docusaurus-social-card.jpg',
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'confval',
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'tutorialSidebar',
          position: 'left',
          label: 'Docs',
        },
        {to: '/releases', label: 'Release', position: 'left'},
        {
          href: 'https://github.com/ethanhann/confval',
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
              label: 'Docs',
              to: '/docs/intro',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'Blog',
              to: '/blog',
            },
            {
              label: 'crates.io',
              href: 'https://crates.io/crates/confval',
            },
            {
              label: 'GitHub',
              href: 'https://github.com/ethanhann/confval',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Ethan Hann. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.gruvboxMaterialLight,
      darkTheme: prismThemes.gruvboxMaterialDark,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
