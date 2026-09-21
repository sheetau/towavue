import { useState } from "react";
import { getInstaller, releaseFallback } from "../site/download.mjs";
import { Icon } from "./Icons";

export function DownloadLink({ content, compact = false }) {
  const [status, setStatus] = useState("idle");

  async function download(event) {
    // Keep the release-page fallback available for ordinary modified link clicks.
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    if (status === "loading") return;
    setStatus("loading");
    try {
      const url = await getInstaller();
      const link = document.createElement("a");
      link.href = url;
      link.download = "";
      document.body.append(link);
      link.click();
      link.remove();
      setStatus("success");
    } catch {
      setStatus("error");
    }
  }

  return (
    <div className={`download-control${compact ? " download-control-compact" : ""}`}>
      <a className={`button app-download${compact ? " button-sm" : ""}`} href={releaseFallback} onClick={download} aria-disabled={status === "loading"} aria-busy={status === "loading"}>
        {status === "loading" ? content.downloading : compact ? content.download : content.downloadWindows}
        {!compact && <Icon name="download" />}
      </a>
      <div className={`download-status${status === "error" ? " is-error" : ""}`} role="status">
        {status === "success" && <span className="sr-only">{content.downloadStarted}</span>}
        {status === "error" && <>{content.downloadError} <a href={releaseFallback} target="_blank" rel="noreferrer">{content.releases} <Icon name="arrow" /></a></>}
      </div>
    </div>
  );
}
