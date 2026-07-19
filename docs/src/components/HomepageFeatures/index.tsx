import type {ReactNode} from 'react';
import clsx from 'clsx';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  description: ReactNode;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'span-first',
    description: (
      <>
        Every parsed value carries the byte range it came from, so any later
        check can point at the exact line and column in the source file.
      </>
    ),
  },
  {
    title: 'one-pass reporting',
    description: (
      <>
        Parsing and validation never stop at the first problem. Issues
        accumulate in a report and the operator sees everything at once.
      </>
    ),
  },
  {
    title: 'format-neutral core',
    description: (
      <>
        Parsing produces a format-neutral field model. HCL and TOML ship today,
        each behind its own feature, and a new format is one more frontend.
      </>
    ),
  },
];

function Feature({title, description}: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
      <div className={styles.card}>
        <Heading as="h3" className={styles.cardTitle}>
          {title}
        </Heading>
        <p className={styles.cardText}>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures(): ReactNode {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
