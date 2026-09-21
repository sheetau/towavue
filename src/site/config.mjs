export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "/towavue";
export const repository = "https://github.com/sheetau/towavue";
export const releaseApi = "https://api.github.com/repos/sheetau/towavue/releases/latest";
export const siteUrl = `https://sheetau.github.io${basePath}`;
export const asset = (path) => `${basePath}/media/${path}`;
export const localePath = (locale) => `${basePath}/${locale === "en" ? "" : `${locale}/`}`;
