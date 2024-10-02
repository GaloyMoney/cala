import clsx from "clsx";
import Heading from "@theme/Heading";
import styles from "./styles.module.css";

type FeatureItem = {
  title: string;
  image: string;
  description: JSX.Element;
  link: string;
};

const FeatureList: FeatureItem[] = [
  {
    title: "GraphQL API",
    image: require("@site/static/img/frontpage-icon-graphql-api.png").default,
    description: (
      <>
        Develop applications efficiently with Cala's GraphQL playground,
        allowing for immediate feedback without preliminary coding. Leverage the
        efficient data fetching to enhance performance and user experience.
      </>
    ),
    link: "https://cala.sh/api-reference.html",
  },
  {
    title: "Double-Entry Accounting",
    image: require("@site/static/img/frontpage-icon-double-entry-accounting.png").default,
    description: (
      <>
        Every transaction is recorded accurately on both sides of the ledger
        providing a complete and transparent view of your financial operations.
      </>
    ),
    link: "/accounting/double-entry-accounting",
  },
  {
    title: "Transaction Templates",
    image: require("@site/static/img/frontpage-icon-transaction-templates.png").default,
    description: (
      <>
        Create custom transaction templates for your specific use cases. Tailor
        each template to fit your unique business needs and streamline your
        financial workflows.
      </>
    ),
    link: "/docs/tx-template-create",
  },
  {
    title: "Embeddable",
    image: require("@site/static/img/frontpage-icon-embeddable.png").default,
    description: (
      <>
        Cala is fully embeddable, capable of being used as a library not
        requiring its own runtime.
      </>
    ),
    link: "https://docs.rs/cala-ledger/latest/cala_ledger/",
  },
  {
    title: "Run Anywhere",
    image: require("@site/static/img/frontpage-icon-run-anywhere.png").default,
    description: (
      <>
        Can serve as a standalone application in the cloud, on your own server,
        or locally as you need.
      </>
    ),
    link: "https://github.com/GaloyMoney/cala?tab=readme-ov-file#cala",
  },
  {
    title: "Open Source Core in Rust",
    image: require("@site/static/img/frontpage-icon-rust.png").default,
    description: (
      <>
        Join our community to contribute and innovate with transparency and
        collaboration at its heart.
      </>
    ),
    link: "https://github.com/GaloyMoney/cala",
  },
];

function Feature({ title, image, description, link }: FeatureItem) {
  return (
    <div className={clsx("col col--4")}>
      <div className="text--center">
        <a href={link} className="noUnderline">
          <img src={image} alt={title} className={styles.featureSvg} />
        </a>
      </div>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">
          <a href={link} className="noUnderline">
            {title}
          </a>
        </Heading>
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
