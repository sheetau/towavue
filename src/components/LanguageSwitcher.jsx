import { languages } from "../site/locales";
import { localePath } from "../site/config.mjs";
import { Icon } from "./Icons";

export function LanguageSwitcher({ locale, label }) {
  return <div className="language-switcher"><label className="sr-only" htmlFor="site-language">{label}</label><select id="site-language" value={locale} onChange={(event) => { window.location.assign(localePath(event.target.value)); }}>{Object.entries(languages).map(([code, name]) => <option key={code} value={code} lang={code}>{name}</option>)}</select><Icon name="chevron" /></div>;
}
