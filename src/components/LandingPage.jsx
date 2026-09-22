import Head from "next/head";
import { asset, localePath, repository, siteUrl } from "../site/config.mjs";
import { languages, locales } from "../site/locales";
import { Logo, Icon } from "./Icons";
import { DownloadLink } from "./DownloadLink";
import { HeroDemo } from "./HeroDemo";
import { FeatureShowcase } from "./FeatureShowcase";
import { LanguageSwitcher } from "./LanguageSwitcher";
import { SourceArtwork } from "./SourceArtwork";

export function LandingPage({ locale = "en" }) {
  const content = locales[locale] ?? locales.en;
  const canonical = `${siteUrl}/${locale === "en" ? "" : `${locale}/`}`;
  const structuredData = { "@context": "https://schema.org", "@type": "SoftwareApplication", name: "towavue", applicationCategory: "MultimediaApplication", operatingSystem: "Windows 11", url: canonical, description: content.description, offers: { "@type": "Offer", price: "0", priceCurrency: "USD" } };

  return <>
    <Head>
      <title>{content.title}</title><meta name="description" content={content.description} />
      <link rel="canonical" href={canonical} />
      {Object.keys(languages).map((code) => <link key={code} rel="alternate" hrefLang={code} href={`${siteUrl}/${code === "en" ? "" : `${code}/`}`} />)}
      <link rel="alternate" hrefLang="x-default" href={`${siteUrl}/`} />
      <link rel="icon" href={asset("favicon.ico")} sizes="any" />
      <meta property="og:type" content="website" /><meta property="og:site_name" content="towavue" /><meta property="og:title" content={content.title} /><meta property="og:description" content={content.description} /><meta property="og:url" content={canonical} /><meta property="og:locale" content={content.ogLocale} /><meta property="og:image" content={`${siteUrl}/media/og-image.png`} /><meta property="og:image:width" content="1200" /><meta property="og:image:height" content="630" /><meta property="og:image:alt" content={content.features[0].alt} /><meta name="twitter:card" content="summary_large_image" />
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(structuredData).replace(/</g, "\\u003c") }} />
    </Head>
    <div className="site-shell">
      <a className="skip-link" href="#main">{content.skip}</a>
      <header className="site-header"><div className="container header-inner"><a className="site-logo" href={localePath(locale)} aria-label={content.home}><Logo /></a><DownloadLink content={content} compact /></div></header>
      <main id="main">
        <section className="hero section" aria-labelledby="hero-title"><div className="container">
          <div className="hero-text"><h1 id="hero-title">{content.hero[0]}{" "}<br />{content.hero[1]}</h1><p>{content.heroDescription}</p><div className="hero-buttons"><DownloadLink content={content} /><span className="platform-note">{content.platform}</span></div></div>
          <HeroDemo content={content.demo} />
        </div></section>
        <section className="feature-block section" aria-labelledby="features-title"><div className="container">
          <div className="section-heading"><h2 id="features-title">{content.featureTitle}</h2><p>{content.featureDescription}</p></div>
          <FeatureShowcase content={content} />
        </div></section>
        <section className="feature-block prefooter section" aria-labelledby="closing-title"><div className="container">
          <div className="section-heading"><h2 id="closing-title">{content.closingTitle}</h2><p>{content.closingDescription}</p></div>
          <div className="closing-grid">{content.closing.map((item) => <article className="closing-card" key={item.title}>
            <div className={`closing-media${item.artwork ? ` closing-media-${item.artwork}` : ""}${item.image || item.artwork ? "" : " is-empty"}`} aria-hidden={!item.image && !item.artwork ? "true" : undefined}>{item.artwork === "source" ? <SourceArtwork label={item.alt} /> : item.image && <img src={asset(item.image)} alt={item.alt} width="740" height="470" loading="lazy" />}</div>
            <div className="closing-copy"><h4>{item.title}</h4><p>{item.description}</p>{item.link && <a className="text-link" href={item.href} target="_blank" rel="noreferrer">{item.link}<Icon name="arrow" /></a>}</div>
          </article>)}</div>
        </div></section>
      </main>
      <footer className="site-footer"><div className="container footer-inner"><span className="copyright">© 2026 sheeta.</span><nav aria-label={locale === "ja" ? "関連リンク" : "Related links"}><a href={repository} target="_blank" rel="noreferrer">GitHub</a><a href={`${repository}/releases`} target="_blank" rel="noreferrer">{content.releases}</a><a href={`${repository}/blob/main/LICENSE-APACHE`} target="_blank" rel="noreferrer">{content.license}</a></nav><LanguageSwitcher locale={locale} label={content.language} /></div></footer>
    </div>
  </>;
}
