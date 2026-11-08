// The synthetic preview and browser acceptance share one loopback-only server.
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export async function createPreviewServer() {
  const root = fileURLToPath(new URL("../dist-preview/", import.meta.url));
  if (!(await stat(root)).isDirectory())
    throw new Error("Build the synthetic preview before serving it");
  const types: Record<string, string> = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".svg": "image/svg+xml",
    ".woff2": "font/woff2",
  };
  const server = createServer(async (request, response) => {
    if (request.method !== "GET" && request.method !== "HEAD") {
      response.writeHead(405, { Allow: "GET, HEAD" }).end();
      return;
    }
    try {
      const name = decodeURIComponent(
        new URL(request.url ?? "/", "http://localhost").pathname,
      );
      const path = resolve(root, `.${name === "/" ? "/index.html" : name}`);
      if (!path.startsWith(resolve(root) + sep)) {
        response.writeHead(400).end();
        return;
      }
      const bytes = await readFile(path);
      response.setHeader(
        "Content-Type",
        types[extname(path)] ?? "application/octet-stream",
      );
      response.setHeader("X-Content-Type-Options", "nosniff");
      response.setHeader("Cache-Control", "no-store");
      response.setHeader(
        "Content-Security-Policy",
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
      );
      response.end(request.method === "HEAD" ? undefined : bytes);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise<void>((done, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      done();
    });
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    server.close();
    throw new Error("No isolated preview listener");
  }
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise<void>((done, reject) =>
        server.close((error) => (error ? reject(error) : done())),
      ),
  };
}

if (import.meta.main) {
  const preview = await createPreviewServer();
  console.log(JSON.stringify({ preview: preview.url, synthetic: true }));
  const stop = () => {
    void preview.close().catch(() => {
      process.exitCode = 1;
    });
  };
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
}
