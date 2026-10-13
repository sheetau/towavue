import "lenis/dist/lenis.css";
import "../styles/globals.css";
import { SmoothScroll } from "../src/components/SmoothScroll";

export default function App({ Component, pageProps }) {
  return <><SmoothScroll /><Component {...pageProps} /></>;
}
