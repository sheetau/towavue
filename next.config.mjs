const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "/towavue";

export default {
  output: "export",
  trailingSlash: true,
  basePath,
  images: { unoptimized: true },
  poweredByHeader: false,
};
