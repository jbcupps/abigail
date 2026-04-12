#!/usr/bin/env node

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..");
const harnessRoot = path.join(repoRoot, "dev-harness");

function parseArgs(argv) {
  const config = {
    port: 43143,
    hiveUrl: "http://127.0.0.1:43141",
    runtimeUrl: "http://127.0.0.1:43142",
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const value = argv[index + 1];
    if (arg === "--port" && value) {
      config.port = Number.parseInt(value, 10);
      index += 1;
    } else if (arg === "--hive-url" && value) {
      config.hiveUrl = value;
      index += 1;
    } else if (arg === "--runtime-url" && value) {
      config.runtimeUrl = value;
      index += 1;
    }
  }

  return config;
}

function contentTypeFor(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  switch (ext) {
    case ".css":
      return "text/css; charset=utf-8";
    case ".js":
      return "application/javascript; charset=utf-8";
    case ".json":
      return "application/json; charset=utf-8";
    case ".html":
    default:
      return "text/html; charset=utf-8";
  }
}

const config = parseArgs(process.argv.slice(2));

const server = http.createServer((request, response) => {
  const url = new URL(request.url ?? "/", `http://127.0.0.1:${config.port}`);

  if (url.pathname === "/config.json") {
    response.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
    response.end(
      JSON.stringify(
        {
          hiveUrl: config.hiveUrl,
          runtimeUrl: config.runtimeUrl,
        },
        null,
        2,
      ),
    );
    return;
  }

  const relativePath = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
  const safePath = path.normalize(relativePath).replace(/^(\.\.[/\\])+/, "");
  const filePath = path.join(harnessRoot, safePath);

  if (!filePath.startsWith(harnessRoot) || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
    response.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
    response.end("Not found");
    return;
  }

  response.writeHead(200, { "Content-Type": contentTypeFor(filePath) });
  fs.createReadStream(filePath).pipe(response);
});

server.listen(config.port, "127.0.0.1", () => {
  console.log(`Browser harness listening on http://127.0.0.1:${config.port}`);
});
