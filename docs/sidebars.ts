import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

/**
 *  The Guide is defined by hand so its pages group into named sections without
 *  a move of any file. Every page URL and every versioned snapshot stays
 *  unchanged.
 */
const sidebars: SidebarsConfig = {
    tutorialSidebar: [
        'getting-started',
        'examples',
        'agent-skills',
        {
            type: 'category',
            label: 'Guide',
            link: {
                type: 'generated-index',
                slug: '/guide',
                description:
                    'confval in depth, grouped by task. Load a file into a typed config, generate and convert configuration files, or connect a schema to an editor.',
            },
            items: [
                {
                    type: 'category',
                    label: 'Load a configuration',
                    collapsed: false,
                    items: [
                        'pipeline',
                        'guide/parsing',
                        'guide/validation',
                        'guide/lowering',
                        'guide/diagnostics',
                        'guide/layering',
                    ],
                },
                {
                    type: 'category',
                    label: 'Generate and convert',
                    collapsed: false,
                    items: [
                        'guide/templates',
                        'guide/representations',
                        'guide/format-limitations',
                    ],
                },
                {
                    type: 'category',
                    label: 'Schema and editors',
                    collapsed: false,
                    items: [
                        'guide/schema-ir',
                        'guide/editor-support',
                        'guide/language-server',
                    ],
                },
            ],
        },
        {
            type: 'category',
            label: 'Internals',
            link: {
                type: 'generated-index',
                slug: '/internals',
                description:
                    'How confval is built: the crates, the modules, and the contribution workflow.',
            },
            items: ['architecture', 'contributing'],
        },
    ],
};

export default sidebars;
