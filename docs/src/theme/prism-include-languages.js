import siteConfig from '@generated/docusaurus.config';

export default function prismIncludeLanguages(PrismObject) {
  const {
    themeConfig: {prism},
  } = siteConfig;
  const {additionalLanguages} = prism;

  globalThis.Prism = PrismObject;

  additionalLanguages.forEach((lang) => {
    // eslint-disable-next-line global-require, import/no-dynamic-require
    require(`prismjs/components/prism-${lang}`);
  });

  // Prism ships no KDL grammar, so a minimal one is defined here: comments,
  // the slashdash marker, strings, keywords, numbers, and structure.
  PrismObject.languages.kdl = {
    comment: [
      {pattern: /\/\/.*/, greedy: true},
      {pattern: /\/\*[\s\S]*?\*\//, greedy: true},
    ],
    slashdash: {pattern: /\/-/, alias: 'comment'},
    string: {pattern: /"(?:\\.|[^"\\])*"/, greedy: true},
    keyword: /#-?(?:true|false|null|inf|nan)\b/,
    number: /[+-]?\b\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?\b/,
    punctuation: /[{}=;]/,
  };

  delete globalThis.Prism;
}
