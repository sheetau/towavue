import { LandingPage } from "../src/components/LandingPage";

export default function HomePage(props) {
  return <LandingPage {...props} />;
}

export function getStaticProps() {
  return { props: { locale: "en" } };
}
