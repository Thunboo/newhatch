const http = require("node:http");

// This receiver belongs only to the export test image. Every replay still
// enters through nginx; production analyzer and capture are not involved.
http.createServer((request, response) => {
  const chunks = [];
  let length = 0;
  request.on("data", (chunk) => {
    length += chunk.length;
    if (length > 1024 * 1024) {
      response.writeHead(413).end();
      request.destroy();
      return;
    }
    chunks.push(chunk);
  });
  request.on("end", () => {
    const body = Buffer.concat(chunks);
    const url = new URL(request.url, "http://fixture");
    response.writeHead(200, { "content-type": "application/json; charset=utf-8" });
    response.end(JSON.stringify({
      method: request.method,
      path: url.pathname,
      params: [...url.searchParams],
      headers: request.headers,
      body: body.toString("utf8"),
      bodyBytes: body.length,
    }));
  });
}).listen(18082, "127.0.0.1");
