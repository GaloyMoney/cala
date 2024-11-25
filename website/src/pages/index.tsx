import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import HomepageFeatures from '@site/src/components/HomepageFeatures';
import Heading from '@theme/Heading';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs/">
            Demo tutorial
          </Link>
        </div>
      </div>
    </header>
  );
}

export default function Home(): JSX.Element {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={`Galoy`}
      description="Description will go into a meta tag in <head />">
      <HomepageHeader />
      <main>
        <HomepageFeatures />

        <div class="container padding-horiz--md">
          <div class="row vert-p">
            <div class="col col--4 padding-horiz--md vert-p">
              <h2>
                The open source core banking ledger
              </h2>
              <p>Cala is an extensible accounting ledger designed for building modern banking applications at enterprise scale.</p>
            </div>
            <div class="col col--8">
              <img src="img/cala.webp" />
            </div>
          </div>

        </div>
      </main>
    </Layout>
  );
}
