import { LandingPage } from "../../src/components/LandingPage";

export default function JapanesePage(props) {
  return <LandingPage {...props} />;
}

export function getStaticProps() {
  return { props: { locale: "ja" } };
}
