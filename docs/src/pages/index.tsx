import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import HomepageFeatures from '@site/src/components/HomepageFeatures';
import Heading from '@theme/Heading';

import styles from './index.module.css';

function HomepageHeader() {
    const {siteConfig} = useDocusaurusContext();
    const site = `${siteConfig.url}${siteConfig.baseUrl}`;
    const workflow = `https://github.com/${siteConfig.organizationName}/${siteConfig.projectName}/actions/workflows/build.yml`;
    const badges = [
        {alt: 'Build', src: `${workflow}/badge.svg?branch=main`},
        {alt: 'Coverage', src: `https://img.shields.io/endpoint?url=${site}coverage/badge.json`},
        {alt: 'Tests', src: `https://img.shields.io/endpoint?url=${site}coverage/tests-badge.json`},
    ];
    return (
        <header className={clsx('hero hero--primary', styles.heroBanner)}>
            <div className="container">
                <div className={styles.heroTitleRow}>
                    <img
                        src={useBaseUrl('/img/logo.svg')}
                        alt="confval logo"
                        className={styles.heroLogo}
                    />
                    <Heading as="h1" className={clsx('hero__title', styles.heroTitle)}>
                        {siteConfig.title}
                    </Heading>
                </div>
                <div className={styles.badges}>
                    {badges.map((badge) => (
                        <a key={badge.alt} href={workflow}>
                            <img src={badge.src} alt={badge.alt}/>
                        </a>
                    ))}
                </div>
                <p className="hero__subtitle">{siteConfig.tagline}</p>
                <p className={styles.heroSummary}>
                    Define a configuration as Rust types, parse a file into them,
                    validate the values, and lower them into the types your
                    program runs on. Errors report the line and column they came
                    from.
                </p>
                <div className={styles.buttons}>
                    <Link
                        className="button button--primary button--lg"
                        to="/docs/getting-started">
                        Get started
                    </Link>
                </div>
            </div>
        </header>
    );
}

export default function Home(): ReactNode {
    const {siteConfig} = useDocusaurusContext();
    return (
        <Layout
            title={siteConfig.title}
            description="A Rust crate for parsing, validating, and lowering configuration files.">
            <HomepageHeader/>
            <main>
                <HomepageFeatures/>
            </main>
        </Layout>
    );
}
