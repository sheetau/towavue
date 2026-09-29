import { getInstaller, releaseFallback } from "../site/download.mjs";

export function DownloadLink({ content, compact = false, status, onStatusChange }) {

  async function download(event) {
    // Keep the release-page fallback available for ordinary modified link clicks.
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    if (status === "loading") return;
    onStatusChange("loading");
    try {
      const url = await getInstaller();
      const link = document.createElement("a");
      link.href = url;
      link.download = "";
      document.body.append(link);
      link.click();
      link.remove();
      onStatusChange("success");
    } catch {
      onStatusChange("error");
    }
  }

  return (
    <a className={`button app-download${compact ? " button-sm" : ""}`} href={releaseFallback} onClick={download} aria-disabled={status === "loading"} aria-busy={status === "loading"}>
      {compact ? content.download : content.downloadWindows}
    </a>
  );
}

export function DownloadStatus({ content, status }) {
  return <span className={`download-status${status === "error" ? " is-error" : ""}`} role="status" aria-atomic="true">
    {status !== "idle" && <span aria-hidden="true"> · </span>}
    {status === "loading" && content.downloading}
    {status === "success" && content.downloadStarted}
    {status === "error" && <>{content.downloadError} <a href={releaseFallback} target="_blank" rel="noreferrer">{content.releases}</a></>}
  </span>;
}
