import {useEffect, useState, type ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Mermaid from '@theme/Mermaid';
import styles from './styles.module.css';
import CodeBlock from "@theme/CodeBlock";
import Tabs from '@theme/Tabs';
import TabItem from '@theme/TabItem';

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

const CONFIG_TOML = `hostname = "127.0.0.1"
port = 8080
allow = ["10.0.0.0/8", "192.168.0.0/16"]

[bind]
port = 8080
`;

const CONFIG_HCL = `hostname = "127.0.0.1"
port     = 8080
allow    = ["10.0.0.0/8", "192.168.0.0/16"]

bind {
  port = 8080
}
`;

const CONFIG_KDL = `hostname "127.0.0.1"
port 8080
allow "10.0.0.0/8" "192.168.0.0/16"

bind {
  port 8080
}
`;

const CONFIG_JSON = `{
  "hostname": "127.0.0.1",
  "port": 8080,
  "allow": ["10.0.0.0/8", "192.168.0.0/16"],
  "bind": { "port": 8080 }
}
`;

const CONFIG_YAML = `hostname: "127.0.0.1"
port: 8080
allow: ["10.0.0.0/8", "192.168.0.0/16"]

bind:
  port: 8080
`;

const TEMPLATE_TOML = `# The host the server binds to.
hostname = "127.0.0.1"

# The port to listen on.
port = 8080

# CIDR ranges allowed to connect.
allow = ["10.0.0.0/8"]

# Connection limits.
[limits]
# Largest request body, in megabytes.
max_body_mb = 16

# Enforcement mode, optional. Defaults to "enforce".
# mode = "enforce"
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
                <h2 className={styles.featureSectionHeader}>Architecture</h2>
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

            <section className={clsx(styles.features, styles.altBgSection)}>
                <div className={styles.featureSectionContent}>
                    <h2 className={styles.featureSectionHeader}>One spec, any format</h2>
                    <p className={styles.lead}>
                        Write the same configuration in TOML, HCL, KDL, JSON, or YAML. Each
                        parses into the identical runtime type, so validation and lowering
                        never depend on which format an operator chose.
                    </p>
                    <Tabs groupId="config-format">
                        <TabItem value="toml" label="TOML">
                            <CodeBlock language="toml">{CONFIG_TOML}</CodeBlock>
                        </TabItem>
                        <TabItem value="hcl" label="HCL">
                            <CodeBlock language="hcl">{CONFIG_HCL}</CodeBlock>
                        </TabItem>
                        <TabItem value="kdl" label="KDL">
                            <CodeBlock language="kdl">{CONFIG_KDL}</CodeBlock>
                        </TabItem>
                        <TabItem value="json" label="JSON">
                            <CodeBlock language="json">{CONFIG_JSON}</CodeBlock>
                        </TabItem>
                        <TabItem value="yaml" label="YAML">
                            <CodeBlock language="yaml">{CONFIG_YAML}</CodeBlock>
                        </TabItem>
                    </Tabs>
                </div>
            </section>

            <section className={styles.features}>
                <div className={styles.featureSectionContent}>
                    <h2 className={styles.featureSectionHeader}>A documented starting point</h2>
                    <p className={styles.lead}>
                        confval generates an annotated template from your spec, with every
                        setting, its doc comment, and each optional field commented out, so an
                        operator starts from a complete file instead of guessing keys.
                    </p>
                    <CodeBlock language="toml">{TEMPLATE_TOML}</CodeBlock>
                </div>
            </section>

            <section className={clsx(styles.features, styles.altBgSection)}>
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
