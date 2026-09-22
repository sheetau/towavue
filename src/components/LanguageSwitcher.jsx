import { useEffect, useRef, useState } from "react";
import { languages } from "../site/locales";
import { localePath } from "../site/config.mjs";

export function LanguageSwitcher({ locale, label }) {
  const [open, setOpen] = useState(false);
  const root = useRef(null);
  const trigger = useRef(null);
  const options = useRef([]);
  const entries = Object.entries(languages);

  useEffect(() => {
    if (!open) return;
    const outside = (event) => { if (!root.current?.contains(event.target)) setOpen(false); };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  function showMenu(index) {
    setOpen(true);
    requestAnimationFrame(() => options.current[index]?.focus());
  }

  function keyDown(event) {
    if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      trigger.current?.focus();
    } else if (open && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const current = options.current.indexOf(document.activeElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? entries.length - 1 : (current + (event.key === "ArrowDown" ? 1 : -1) + entries.length) % entries.length;
      options.current[next]?.focus();
    }
  }

  return <div ref={root} className={`language-switcher${open ? " is-open" : ""}`} onKeyDown={keyDown} onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false); }}>
    <button ref={trigger} className="language-switcher-button" type="button" aria-label={label} aria-haspopup="menu" aria-expanded={open} aria-controls="language-menu" onClick={() => open ? setOpen(false) : showMenu(entries.findIndex(([code]) => code === locale))} onKeyDown={(event) => {
      if (!open && ["ArrowDown", "ArrowUp"].includes(event.key)) {
        event.preventDefault();
        event.stopPropagation();
        showMenu(event.key === "ArrowDown" ? 0 : entries.length - 1);
      }
    }}>
      <span>{languages[locale]}</span><svg viewBox="0 0 8 13" fill="none" aria-hidden="true"><path d="M1 12L7 6.5L1 1" stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round" /></svg>
    </button>
    <div id="language-menu" className="language-switcher-menu" role="menu" aria-label={label} hidden={!open}>
      {entries.map(([code, name], index) => <a key={code} ref={(element) => { options.current[index] = element; }} className="language-switcher-option" href={localePath(code)} hrefLang={code} role="menuitemradio" aria-checked={locale === code} tabIndex={-1} lang={code} onKeyDown={(event) => {
        if (event.key === " ") { event.preventDefault(); event.currentTarget.click(); }
      }} onClick={(event) => {
        try { localStorage.setItem("towavue-locale", code); } catch { /* Language links also work with storage disabled. */ }
        if (code === locale && !event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey) {
          event.preventDefault(); setOpen(false); trigger.current?.focus();
        }
      }}><span>{name}</span><span className="language-switcher-check" aria-hidden="true">✓</span></a>)}
    </div>
  </div>;
}
