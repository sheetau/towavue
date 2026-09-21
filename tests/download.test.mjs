import test from "node:test";
import assert from "node:assert/strict";
import { getInstaller, selectInstaller } from "../src/site/download.mjs";

const asset = (name, url = `https://github.com/sheetau/towavue/releases/download/v1.0.1/${name}`) => ({ name, browser_download_url: url });
const installer = asset("towavue-1.0.1-windows-x64-setup.exe");

test("selects the x64 Setup, not sources, signatures, or another executable", () => {
  const result = selectInstaller({ assets: [asset("towavue-1.0.1-sources.zip"), asset("vc_redist.x64.exe"), asset("towavue-1.0.1-windows-arm64-setup.exe"), installer] });
  assert.equal(result, installer);
});
test("refuses absent installers, malformed responses, drafts, and prereleases", () => {
  for (const release of [null, {}, { assets: [] }, { assets: [installer], draft: true }, { assets: [installer], prerelease: true }]) assert.equal(selectInstaller(release), null);
});
test("refuses unexpected asset hosts and repositories", () => {
  for (const url of ["https://example.com/setup.exe", "https://github.com/other/app/releases/download/v1.0.1/towavue-1.0.1-windows-x64-setup.exe", "javascript:alert(1)"]) assert.equal(selectInstaller({ assets: [asset(installer.name, url)] }), null);
});
test("resolves the current release and reports API errors or missing assets", async () => {
  assert.equal(await getInstaller(async () => ({ ok: true, json: async () => ({ assets: [installer] }) })), installer.browser_download_url);
  await assert.rejects(getInstaller(async () => ({ ok: false, status: 403 })), /403/);
  await assert.rejects(getInstaller(async () => ({ ok: true, json: async () => ({ assets: [] }) })), /not found/);
  await assert.rejects(getInstaller(async () => { throw new Error("offline"); }), /offline/);
});
