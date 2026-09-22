const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "/towavue";

export default {
  output: "export",
  distDir: process.env.NODE_ENV === "development" ? ".next-dev" : ".next",
  trailingSlash: true,
  basePath,
  images: { unoptimized: true },
  poweredByHeader: false,
};
