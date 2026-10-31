import Head from "next/head";
import Script from "next/script";
import { useEffect, useState } from "react";
import { analyticsId, asset, basePath, localePath, repository, siteUrl } from "../site/config.mjs";
import { languages, locales } from "../site/locales";
import { HeaderLogo } from "./HeaderLogo";
import { AmbientFish } from "./AmbientFish";
import { DownloadLink, DownloadStatus } from "./DownloadLink";
import { HeroDemo } from "./HeroDemo";
import { Benchmark } from "./Benchmark";
import { TimelineDemo } from "./TimelineDemo";
import { FilmstripDemo } from "./FilmstripDemo";
import { ScrollProgress } from "./ScrollProgress";
import media from "../site/gallery.json";
import { LanguageSwitcher } from "./LanguageSwitcher";


function SectionHeading({ id, content, locale }) {
  return <div className="section-heading">
    <div className="section-label">{content.label}</div>
    <h2 id={id}>{content.title}</h2>
    <p>{content.description}</p>
  </div>;
}

function FeatureColumns({ items }) {
  return <div className="feature-grid">{items.map((item) => <article className="feature-column" key={item.title}>
    <h3>{item.title}</h3><p>{item.description}</p>
  </article>)}</div>;
}

export function LandingPage({ locale = "en" }) {
  const [downloadStatus, setDownloadStatus] = useState("idle");
  useEffect(() => {
    if (downloadStatus !== "success" && downloadStatus !== "error") return;
    // Native download dialogs expose no completion/cancellation event to this page.
    const timeout = window.setTimeout(() => setDownloadStatus("idle"), downloadStatus === "success" ? 5000 : 10000);
    return () => window.clearTimeout(timeout);
  }, [downloadStatus]);
  const content = locales[locale] ?? locales.en;
  const canonical = `${siteUrl}/${locale === "en" ? "" : `${locale}/`}`;
  const structuredData = {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: "towavue",
    alternateName: content.alternateName,
    inLanguage: locale,
    applicationCategory: "MultimediaApplication",
    operatingSystem: "Windows 11",
    url: canonical,
    description: content.description,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  };

  return (
    <>
      <Head>
        <title>{content.title}</title>
        <meta name="description" content={content.description} />
        {content.keywords && <meta name="keywords" content={content.keywords} />}
        <link rel="canonical" href={canonical} />
        {Object.keys(languages).map((code) => (
          <link key={code} rel="alternate" hrefLang={code} href={`${siteUrl}/${code === "en" ? "" : `${code}/`}`} />
        ))}
        <link rel="alternate" hrefLang="x-default" href={`${siteUrl}/`} />
        <link rel="icon" href={asset("favicon.ico")} sizes="any" />
        <meta property="og:type" content="website" />
        <meta property="og:site_name" content="towavue" />
        <meta property="og:title" content={content.title} />
        <meta property="og:description" content={content.description} />
        <meta property="og:url" content={canonical} />
        <meta property="og:locale" content={content.ogLocale} />
        <meta property="og:image" content={`${siteUrl}/media/towavue-product-preview.png`} />
        <meta property="og:image:secure_url" content={`${siteUrl}/media/towavue-product-preview.png`} />
        <meta property="og:image:type" content="image/png" />
        <meta property="og:image:width" content="997" />
        <meta property="og:image:height" content="622" />
        <meta property="og:image:alt" content={content.shareImageAlt} />
        <meta name="twitter:card" content="summary_large_image" />
        <meta name="twitter:image" content={`${siteUrl}/media/towavue-product-preview.png`} />
        <meta name="twitter:image:alt" content={content.shareImageAlt} />
        <meta name="google-site-verification" content="CbYcYiYRBtqbBzRcFrgow4wD0aZR5AceysXb3puyV5M" />
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(structuredData).replace(/</g, "\\u003c") }}
        />
      </Head>
      <Script id="google-analytics" strategy="afterInteractive">
        {`
          window.dataLayer = window.dataLayer || [];
          function gtag(){window.dataLayer.push(arguments);}
          gtag("js", new Date());
          if (location.hostname === "sheetau.github.io") {
            gtag("config", ${JSON.stringify(analyticsId)});
            const script = document.createElement("script");
            script.src = "https://www.googletagmanager.com/gtag/js?id=" + ${JSON.stringify(analyticsId)};
            script.async = true;
            document.head.append(script);
          }
        `}
      </Script>
      <AmbientFish />
      <div className="site-shell">
        <a className="skip-link" href="#main">
          {content.skip}
        </a>
        <header className="site-header">
          <div className="header-inner">
            <HeaderLogo href={localePath(locale)} label={content.home} />
            <a className="site-wordmark" href={localePath(locale)} aria-label={content.home}>towavue</a>
            <DownloadLink content={content} compact status={downloadStatus} onStatusChange={setDownloadStatus} />
          </div>
          <ScrollProgress />
        </header>
        <main id="main" className="container main-content">
          <section className="hero section" aria-labelledby="hero-title">
            <div className="hero-text">
              <h1 id="hero-title">{content.hero}</h1>
              <p>{content.heroDescription}</p>
            </div>
            <div className="hero-buttons">
              <div className="hero-actions">
                <DownloadLink content={content} status={downloadStatus} onStatusChange={setDownloadStatus} />
                <a className="button github-star" href={repository} target="_blank" rel="noreferrer">Star on GitHub</a>
              </div>
            </div>
            <HeroDemo content={content.demo} />
          </section>
          <section className="section benchmark-section" aria-labelledby="benchmark-title">
            <SectionHeading id="benchmark-title" content={content.benchmark} locale={locale} />
            <Benchmark content={content.benchmark} locale={locale} />
          </section>
          <section className="section workspace-section" aria-labelledby="workspace-title">
            <SectionHeading id="workspace-title" content={content.workspace} locale={locale} />
            <div className="feature-grid workspace-grid">{content.workspace.items.map((item, index) => {
              const picture = media.artwork[`workspace${index + 1}`];
              return <article className="feature-column" key={item.title}>
                <img className="workspace-image" src={asset(picture.src)} width={picture.width} height={picture.height} alt={item.alt} loading="lazy" />
                <h3>{item.title}</h3><p>{item.description}</p>
              </article>;
            })}</div>
          </section>
          <section className="section editing-section" aria-labelledby="editing-title">
            <SectionHeading id="editing-title" content={content.editing} locale={locale} />
            <TimelineDemo content={content.timeline} />
            <FeatureColumns items={content.editing.items} />
          </section>
          <section className="section discovery-section" aria-labelledby="discovery-title">
            <SectionHeading id="discovery-title" content={content.discovery} locale={locale} />
            <FilmstripDemo content={content.filmstrip} />
            <FeatureColumns items={content.discovery.items} />
          </section>
          <section className="section final-download" aria-labelledby="closing-title">
            <h2 id="closing-title">{content.closingTitle}</h2>
            <DownloadLink content={content} status={downloadStatus} onStatusChange={setDownloadStatus} />
          </section>
        </main>
        <div className={`download-feedback${downloadStatus === "idle" ? " is-idle" : ""}`}><DownloadStatus content={content} status={downloadStatus} /></div>
        <footer className="site-footer">
          <div className="container footer-inner">
            <span className="copyright">© 2026 sheeta.</span>
            <nav aria-label={locale === "ja" ? "関連リンク" : "Related links"}>
              <a href={repository} target="_blank" rel="noreferrer">
                GitHub
              </a>
              <a href={`${repository}/releases`} target="_blank" rel="noreferrer">
                {content.releases}
              </a>
              <a href={`${repository}/blob/main/LICENSE-APACHE`} target="_blank" rel="noreferrer">
                {content.license}
              </a>
            </nav>
            <a className="image-credits-link" href={`${basePath}/image-credits/`}>{locale === "ja" ? "画像の出典" : "Image credits"}</a>
            <LanguageSwitcher locale={locale} label={content.language} />
          </div>
        </footer>
      </div>
    </>
  );
}
