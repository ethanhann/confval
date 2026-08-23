import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'confval',
  tagline: 'A batteries-included configuration toolkit for Rust',
  favicon: 'img/logo.svg',

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

  markdown: {
    mermaid: true,
  },

  themes: ['@docusaurus/theme-mermaid'],

  plugins: [
    [
      'docusaurus-plugin-llms',
      {
        excludeImports: true,
        docsDir: [
          { path: 'docs', routeBasePath: 'docs', label: 'Docs' },
          { path: 'releases', routeBasePath: 'releases', label: 'Releases' },
        ],
      },
    ],
    [
      '@docusaurus/plugin-content-blog',
      {
        id: 'releases',
        routeBasePath: 'releases',
        path: './releases',
        blogTitle: 'Releases',
        blogDescription: 'confval release notes and changelogs.',
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
          lastVersion: '0.9.0',
          versions: {
            current: {
              label: '0.9.x-dev',
              banner: 'unreleased',
            },
          },
        },
        blog: false,
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
    mermaid: {
      theme: {light: 'neutral', dark: 'dark'},
      options: {
        fontFamily:
          'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", "DejaVu Sans Mono", monospace',
      },
    },
    navbar: {
      title: 'confval',
      logo: {
        alt: 'confval logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'tutorialSidebar',
          position: 'left',
          label: 'Docs',
        },
        {to: '/releases', label: 'Releases', position: 'left'},
        {
          type: 'docsVersionDropdown',
          position: 'right',
        },
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
              to: '/docs/getting-started',
            },
            {
              label: 'llms.txt',
              href: 'https://ethanhann.com/confval/llms.txt',
            },
            {
              label: 'llms-full.txt',
              href: 'https://ethanhann.com/confval/llms-full.txt',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'Releases',
              to: '/releases',
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
      additionalLanguages: ['toml', 'hcl', 'json', 'yaml'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
