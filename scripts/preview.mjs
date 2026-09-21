import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import path from "node:path";
import { basePath } from "../src/site/config.mjs";

const root = path.resolve("out");
const port = Number(process.env.PORT ?? 3000);
const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".json": "application/json", ".webp": "image/webp", ".png": "image/png", ".ico": "image/x-icon", ".mp4": "video/mp4", ".xml": "application/xml", ".txt": "text/plain; charset=utf-8" };

createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://localhost");
    if (basePath && url.pathname === "/") { response.writeHead(302, { Location: `${basePath}/` }).end(); return; }
    if (basePath && url.pathname !== basePath && !url.pathname.startsWith(`${basePath}/`)) throw new Error("Not found");
    const relative = decodeURIComponent(url.pathname.slice(basePath.length));
    let filename = path.resolve(root, `.${relative || "/"}`);
    if (filename !== root && !filename.startsWith(`${root}${path.sep}`)) throw new Error("Not found");
    let info = await stat(filename);
    if (info.isDirectory()) { filename = path.join(filename, "index.html"); info = await stat(filename); }
    const headers = { "Content-Type": types[path.extname(filename)] ?? "application/octet-stream", "Accept-Ranges": "bytes" };
    const range = request.headers.range?.match(/^bytes=(\d+)-(\d*)$/);
    let start = 0;
    let end = info.size - 1;
    if (range) {
      start = Number(range[1]);
      end = range[2] ? Math.min(Number(range[2]), end) : end;
      if (start > end || start >= info.size) { response.writeHead(416, { "Content-Range": `bytes */${info.size}` }).end(); return; }
      headers["Content-Range"] = `bytes ${start}-${end}/${info.size}`;
    }
    headers["Content-Length"] = end - start + 1;
    response.writeHead(range ? 206 : 200, headers);
    if (request.method === "HEAD") response.end();
    else createReadStream(filename, { start, end }).pipe(response);
  } catch { response.writeHead(404).end("Not found"); }
}).listen(port, "127.0.0.1", () => console.log(`Preview: http://localhost:${port}${basePath}/`));
