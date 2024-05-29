import clsx from 'clsx';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  image: string;
  description: JSX.Element;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'GraphQL API',
    image: require('@site/static/img/graphql-api-logo.png').default,
    description: (
      <>
        Test and refine your applications efficiently with Cala's local GraphQL Playground, allowing for immediate feedback without preliminary coding. Leverage GraphQL's efficient data fetching to enhance performance and user experience.
      </>
    ),
  },
  {
    title: 'Double Sided Accounting',
    image: require('@site/static/img/double-sided-accounting-logo.png').default,
    description: (
      <>
        Every transaction is recorded accurately on both sides of the ledger providing a complete and transparent view of your financial operations.
      </>
    ),
  },
  {
    title: 'Run Anywhere',
    image: require('@site/static/img/run-anywhere-logo.png').default,
    description: (
      <>
        Can serve you in the cloud, on your own server or locally as you need.
      </>
    ),
  },
  {
    title: 'Transaction Templates',
    image: require('@site/static/img/financial-operation-logo.png').default,
    description: (
      <>
        Create custom transaction templates for your specific use cases.
      </>
    ),
  },
  {
    title: 'Embeddable',
    image: require('@site/static/img/embeddable-logo.png').default,
    description: (
      <>
        Cala is fully embeddable, allowing it to seamlessly integrate into any software, enhancing your existing systems.
      </>
    ),
  },

  {
    title: 'Open Source Core in Rust',
    image: require('@site/static/img/rust-logo.png').default,
    description: (
      <>
        Join our community to contribute and innovate with transparency and collaboration at its heart.
      </>
    ),
  },
];

function Feature({ title, image, description }: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center">
        <img src={image} alt={title} className={styles.featureSvg} />
      </div>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures(): JSX.Element {
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
