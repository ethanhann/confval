import {useEffect, useState, type ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Mermaid from '@theme/Mermaid';
import styles from './styles.module.css';
import CodeBlock from "@theme/CodeBlock";

const diagram = (direction: string, nodeSpacing: number) => `%%{ init: { "flowchart": { "curve": "basis", "nodeSpacing": ${nodeSpacing} } } }%%
flowchart ${direction}
    file[/"<b>Config file</b><br/>TOML, HCL, KDL, JSON, or YAML"/]
    parse["parse"]
    validate["validate"]
    gate{"errors?"}
    lower["lower"]
    stop(["<b>Stop</b><br/>render diagnostics"])
    config(["<b>Config types</b><br/>runtime form"])

    file --> parse --> validate --> gate
    gate -- yes --> stop
    gate -- no --> lower --> config

    classDef io stroke:#928374,stroke-width:1.5px;
    classDef step stroke:#5aa469,stroke-width:1.5px;
    classDef decide stroke:#d79921,stroke-width:1.5px;
    classDef bad stroke:#cc4b37,stroke-width:1.5px;

    class file io;
    class parse,validate,lower,config step;
    class gate decide;
    class stop bad;`;

const PIPELINE_WIDE = diagram('LR', 100);
const PIPELINE_TALL = diagram('TD', 50);

const INVALID_TOML = `port = 80
tls = true
allow = ["10.0.0.0/8", ""]

[limits]
mode = "yolo"
`;

// Flow left to right on wide screens, top to bottom on medium and smaller ones.
function useWideViewport(): boolean {
    const [wide, setWide] = useState(false);
    useEffect(() => {
        const query = window.matchMedia('(min-width: 1200px)');
        const update = () => setWide(query.matches);
        update();
        query.addEventListener('change', update);
        return () => query.removeEventListener('change', update);
    }, []);
    return wide;
}

export default function HomepageFeatures(): ReactNode {
    const wide = useWideViewport();
    return (
        <>
            <section className={styles.features}>
                <p className={styles.lead}>
                    confval turns a configuration file into runtime types in four stages:
                    parse, validate, gate, and lower.
                </p>
                <div className={clsx(styles.diagram, wide ? styles.wide : styles.tall)}>
                    <Mermaid value={wide ? PIPELINE_WIDE : PIPELINE_TALL}/>
                </div>
                <p className={styles.caption}>
                    See <Link to="/docs/pipeline">The pipeline contract</Link> for more architecture details.
                </p>
            </section>

            <section className={clsx(styles.features, styles.screenshotSection)}>
                <div className={styles.featureSectionContent}>
                    <h2 className={styles.featureSectionHeader}>Diagnostics</h2>
                    <p className={styles.lead}>
                        confval produces operator-friendly, accumulated validation diagnostics in a variety of formats
                        (i.e., pretty, plain, and JSON).
                    </p>

                    <h3 className={styles.stepLabel}>This invalid TOML...</h3>
                    <CodeBlock language="toml">{INVALID_TOML}</CodeBlock>

                    <h3 className={styles.stepLabel}>Produces these diagnostics...</h3>
                    <img
                        src={useBaseUrl('/img/invalid_toml_pretty_output_example.png')}
                        alt="Example of pretty-formatted validation diagnostics for invalid TOML configuration"
                    />
                </div>
            </section>
        </>
    );
}
