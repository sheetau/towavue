import { releaseApi, repository } from "./config.mjs";

export function selectInstaller(release) {
  if (release?.draft || release?.prerelease || !Array.isArray(release?.assets)) return null;
  return release.assets.find(({ name, browser_download_url: url }) => {
    if (!/^towavue-.+-windows-x64-setup\.exe$/i.test(name ?? "")) return false;
    try {
      const parsed = new URL(url);
      return parsed.origin === "https://github.com" && parsed.pathname.startsWith("/sheetau/towavue/releases/download/") && parsed.pathname.endsWith(`/${name}`);
    } catch { return false; }
  }) ?? null;
}

export async function getInstaller(fetcher = fetch) {
  const response = await fetcher(releaseApi, {
    headers: { Accept: "application/vnd.github+json" },
    signal: AbortSignal.timeout(12000),
  });
  if (!response.ok) throw new Error(`Release lookup failed: ${response.status}`);
  const installer = selectInstaller(await response.json());
  if (!installer) throw new Error("Windows installer not found");
  return installer.browser_download_url;
}

export const releaseFallback = `${repository}/releases/latest`;
