// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

import EventEmitter from "node:events";
import http, { type RequestOptions } from "node:http";
import https from "node:https";
import {
  assert,
  assertEquals,
} from "../../../test_util/std/testing/asserts.ts";
import { assertSpyCalls, spy } from "../../../test_util/std/testing/mock.ts";
import { deferred } from "../../../test_util/std/async/deferred.ts";

import { gzip } from "node:zlib";
import { Buffer } from "node:buffer";
import { serve } from "../../../test_util/std/http/server.ts";
import { execCode } from "../unit/test_util.ts";

Deno.test("[node/http listen]", async () => {
  {
    const server = http.createServer();
    assertEquals(0, EventEmitter.listenerCount(server, "request"));
  }

  {
    const server = http.createServer(() => {});
    assertEquals(1, EventEmitter.listenerCount(server, "request"));
  }

  {
    const promise = deferred<void>();
    const server = http.createServer();

    server.listen(() => {
      server.close();
    });
    server.on("close", () => {
      promise.resolve();
    });

    await promise;
  }

  {
    const promise = deferred<void>();
    const server = http.createServer();

    server.listen().on("listening", () => {
      server.close();
    });
    server.on("close", () => {
      promise.resolve();
    });

    await promise;
  }

  for (const port of [0, -0, 0.0, "0", null, undefined]) {
    const promise = deferred<void>();
    const server = http.createServer();

    server.listen(port, () => {
      server.close();
    });
    server.on("close", () => {
      promise.resolve();
    });

    await promise;
  }
});

Deno.test("[node/http close]", async () => {
  {
    const promise1 = deferred<void>();
    const promise2 = deferred<void>();
    // Node quirk: callback gets exception object, event listener does not.
    // deno-lint-ignore no-explicit-any
    const server = http.createServer().close((err: any) => {
      assertEquals(err.code, "ERR_SERVER_NOT_RUNNING");
      promise1.resolve();
    });
    // deno-lint-ignore no-explicit-any
    server.on("close", (err: any) => {
      assertEquals(err, undefined);
      promise2.resolve();
    });
    server.on("listening", () => {
      throw Error("unreachable");
    });
    await promise1;
    await promise2;
  }

  {
    const promise1 = deferred<void>();
    const promise2 = deferred<void>();
    const server = http.createServer().listen().close((err) => {
      assertEquals(err, undefined);
      promise1.resolve();
    });
    // deno-lint-ignore no-explicit-any
    server.on("close", (err: any) => {
      assertEquals(err, undefined);
      promise2.resolve();
    });
    server.on("listening", () => {
      throw Error("unreachable");
    });
    await promise1;
    await promise2;
  }
});

Deno.test("[node/http] chunked response", async () => {
  for (
    const body of [undefined, "", "ok"]
  ) {
    const expected = body ?? "";
    const promise = deferred<void>();

    const server = http.createServer((_req, res) => {
      res.writeHead(200, { "transfer-encoding": "chunked" });
      res.end(body);
    });

    server.listen(async () => {
      const res = await fetch(
        // deno-lint-ignore no-explicit-any
        `http://127.0.0.1:${(server.address() as any).port}/`,
      );
      assert(res.ok);

      const actual = await res.text();
      assertEquals(actual, expected);

      server.close(() => promise.resolve());
    });

    await promise;
  }
});

// TODO(kt3k): This test case exercises the workaround for https://github.com/denoland/deno/issues/17194
// This should be removed when #17194 is resolved.
Deno.test("[node/http] empty chunk in the middle of response", async () => {
  const promise = deferred<void>();

  const server = http.createServer((_req, res) => {
    res.write("a");
    res.write("");
    res.write("b");
    res.end();
  });

  server.listen(async () => {
    const res = await fetch(
      // deno-lint-ignore no-explicit-any
      `http://127.0.0.1:${(server.address() as any).port}/`,
    );
    const actual = await res.text();
    assertEquals(actual, "ab");
    server.close(() => promise.resolve());
  });

  await promise;
});

Deno.test("[node/http] server can respond with 101, 204, 205, 304 status", async () => {
  for (const status of [101, 204, 205, 304]) {
    const promise = deferred<void>();
    const server = http.createServer((_req, res) => {
      res.statusCode = status;
      res.end("");
    });
    server.listen(async () => {
      const res = await fetch(
        // deno-lint-ignore no-explicit-any
        `http://127.0.0.1:${(server.address() as any).port}/`,
      );
      await res.arrayBuffer();
      assertEquals(res.status, status);
      server.close(() => promise.resolve());
    });
    await promise;
  }
});

Deno.test("[node/http] request default protocol", async () => {
  const promise = deferred<void>();
  const promise2 = deferred<void>();
  const server = http.createServer((_, res) => {
    res.end("ok");
  });

  // @ts-ignore IncomingMessageForClient
  // deno-lint-ignore no-explicit-any
  let clientRes: any;
  server.listen(() => {
    const req = http.request(
      // deno-lint-ignore no-explicit-any
      { host: "localhost", port: (server.address() as any).port },
      (res) => {
        assertEquals(res.complete, false);
        res.on("data", () => {});
        res.on("end", () => {
          server.close();
        });
        clientRes = res;
        assertEquals(res.statusCode, 200);
        promise2.resolve();
      },
    );
    req.end();
  });
  server.on("close", () => {
    promise.resolve();
  });
  await promise;
  await promise2;
  assertEquals(clientRes!.complete, true);
});

Deno.test("[node/http] request with headers", async () => {
  const promise = deferred<void>();
  const server = http.createServer((req, res) => {
    assertEquals(req.headers["x-foo"], "bar");
    res.end("ok");
  });
  server.listen(() => {
    const req = http.request(
      {
        host: "localhost",
        // deno-lint-ignore no-explicit-any
        port: (server.address() as any).port,
        headers: { "x-foo": "bar" },
      },
      (res) => {
        res.on("data", () => {});
        res.on("end", () => {
          server.close();
        });
        assertEquals(res.statusCode, 200);
      },
    );
    req.end();
  });
  server.on("close", () => {
    promise.resolve();
  });
  await promise;
});

Deno.test("[node/http] non-string buffer response", {
  // TODO(kt3k): Enable sanitizer. A "zlib" resource is leaked in this test case.
  sanitizeResources: false,
}, async () => {
  const promise = deferred<void>();
  const server = http.createServer((_, res) => {
    gzip(
      Buffer.from("a".repeat(100), "utf8"),
      {},
      (_err: Error | null, data: Buffer) => {
        res.setHeader("Content-Encoding", "gzip");
        res.end(data);
      },
    );
  });
  server.listen(async () => {
    const res = await fetch(
      // deno-lint-ignore no-explicit-any
      `http://localhost:${(server.address() as any).port}`,
    );
    try {
      const text = await res.text();
      assertEquals(text, "a".repeat(100));
    } catch (e) {
      server.emit("error", e);
    } finally {
      server.close(() => promise.resolve());
    }
  });
  await promise;
});

// TODO(kt3k): Enable this test
// Currently ImcomingMessage constructor has incompatible signature.
/*
Deno.test("[node/http] http.IncomingMessage can be created without url", () => {
  const message = new http.IncomingMessage(
    // adapted from https://github.com/dougmoscrop/serverless-http/blob/80bfb3e940057d694874a8b0bc12ad96d2abe7ab/lib/request.js#L7
    {
      // @ts-expect-error - non-request properties will also be passed in, e.g. by serverless-http
      encrypted: true,
      readable: false,
      remoteAddress: "foo",
      address: () => ({ port: 443 }),
      // deno-lint-ignore no-explicit-any
      end: Function.prototype as any,
      // deno-lint-ignore no-explicit-any
      destroy: Function.prototype as any,
    },
  );
  message.url = "https://example.com";
});
*/

Deno.test("[node/http] send request with non-chunked body", async () => {
  let requestHeaders: Headers;
  let requestBody = "";

  const hostname = "localhost";
  const port = 4505;

  // NOTE: Instead of node/http.createServer(), serve() in std/http/server.ts is used.
  // https://github.com/denoland/deno_std/pull/2755#discussion_r1005592634
  const handler = async (req: Request) => {
    requestHeaders = req.headers;
    requestBody = await req.text();
    return new Response("ok");
  };
  const abortController = new AbortController();
  const servePromise = serve(handler, {
    hostname,
    port,
    signal: abortController.signal,
    onListen: undefined,
  });

  const opts: RequestOptions = {
    host: hostname,
    port,
    method: "POST",
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Content-Length": "11",
    },
  };
  const req = http.request(opts, (res) => {
    res.on("data", () => {});
    res.on("end", () => {
      abortController.abort();
    });
    assertEquals(res.statusCode, 200);
    assertEquals(requestHeaders.get("content-length"), "11");
    assertEquals(requestHeaders.has("transfer-encoding"), false);
    assertEquals(requestBody, "hello world");
  });
  req.write("hello ");
  req.write("world");
  req.end();

  await servePromise;
});

Deno.test("[node/http] send request with chunked body", async () => {
  let requestHeaders: Headers;
  let requestBody = "";

  const hostname = "localhost";
  const port = 4505;

  // NOTE: Instead of node/http.createServer(), serve() in std/http/server.ts is used.
  // https://github.com/denoland/deno_std/pull/2755#discussion_r1005592634
  const handler = async (req: Request) => {
    requestHeaders = req.headers;
    requestBody = await req.text();
    return new Response("ok");
  };
  const abortController = new AbortController();
  const servePromise = serve(handler, {
    hostname,
    port,
    signal: abortController.signal,
    onListen: undefined,
  });

  const opts: RequestOptions = {
    host: hostname,
    port,
    method: "POST",
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Content-Length": "11",
      "Transfer-Encoding": "chunked",
    },
  };
  const req = http.request(opts, (res) => {
    res.on("data", () => {});
    res.on("end", () => {
      abortController.abort();
    });
    assertEquals(res.statusCode, 200);
    assertEquals(requestHeaders.has("content-length"), false);
    assertEquals(requestHeaders.get("transfer-encoding"), "chunked");
    assertEquals(requestBody, "hello world");
  });
  req.write("hello ");
  req.write("world");
  req.end();

  await servePromise;
});

Deno.test("[node/http] send request with chunked body as default", async () => {
  let requestHeaders: Headers;
  let requestBody = "";

  const hostname = "localhost";
  const port = 4505;

  // NOTE: Instead of node/http.createServer(), serve() in std/http/server.ts is used.
  // https://github.com/denoland/deno_std/pull/2755#discussion_r1005592634
  const handler = async (req: Request) => {
    requestHeaders = req.headers;
    requestBody = await req.text();
    return new Response("ok");
  };
  const abortController = new AbortController();
  const servePromise = serve(handler, {
    hostname,
    port,
    signal: abortController.signal,
    onListen: undefined,
  });

  const opts: RequestOptions = {
    host: hostname,
    port,
    method: "POST",
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
    },
  };
  const req = http.request(opts, (res) => {
    res.on("data", () => {});
    res.on("end", () => {
      abortController.abort();
    });
    assertEquals(res.statusCode, 200);
    assertEquals(requestHeaders.has("content-length"), false);
    assertEquals(requestHeaders.get("transfer-encoding"), "chunked");
    assertEquals(requestBody, "hello world");
  });
  req.write("hello ");
  req.write("world");
  req.end();

  await servePromise;
});

Deno.test("[node/http] ServerResponse _implicitHeader", async () => {
  const d = deferred<void>();
  const server = http.createServer((_req, res) => {
    const writeHeadSpy = spy(res, "writeHead");
    // deno-lint-ignore no-explicit-any
    (res as any)._implicitHeader();
    assertSpyCalls(writeHeadSpy, 1);
    writeHeadSpy.restore();
    res.end("Hello World");
  });

  server.listen(async () => {
    const { port } = server.address() as { port: number };
    const res = await fetch(`http://localhost:${port}`);
    assertEquals(await res.text(), "Hello World");
    server.close(() => {
      d.resolve();
    });
  });

  await d;
});

Deno.test("[node/http] server unref", async () => {
  const [statusCode, _output] = await execCode(`
  import http from "node:http";
  const server = http.createServer((_req, res) => {
    res.statusCode = status;
    res.end("");
  });

  // This should let the program to exit without waiting for the
  // server to close.
  server.unref();

  server.listen(async () => {
  });
  `);
  assertEquals(statusCode, 0);
});

Deno.test("[node/http] ClientRequest handle non-string headers", async () => {
  // deno-lint-ignore no-explicit-any
  let headers: any;
  const def = deferred();
  const req = http.request("http://localhost:4545/echo_server", {
    method: "POST",
    headers: { 1: 2 },
  }, (resp) => {
    headers = resp.headers;

    resp.on("data", () => {});

    resp.on("end", () => {
      def.resolve();
    });
  });
  req.once("error", (e) => def.reject(e));
  req.end();
  await def;
  assertEquals(headers!["1"], "2");
});

Deno.test("[node/http] ClientRequest uses HTTP/1.1", async () => {
  let body = "";
  const def = deferred();
  const req = https.request("https://localhost:5545/http_version", {
    method: "POST",
    headers: { 1: 2 },
  }, (resp) => {
    resp.on("data", (chunk) => {
      body += chunk;
    });

    resp.on("end", () => {
      def.resolve();
    });
  });
  req.once("error", (e) => def.reject(e));
  req.end();
  await def;
  assertEquals(body, "HTTP/1.1");
});

Deno.test("[node/http] ClientRequest setTimeout", async () => {
  let body = "";
  const def = deferred();
  const timer = setTimeout(() => def.reject("timed out"), 50000);
  const req = http.request("http://localhost:4545/http_version", (resp) => {
    resp.on("data", (chunk) => {
      body += chunk;
    });

    resp.on("end", () => {
      def.resolve();
    });
  });
  req.setTimeout(120000);
  req.once("error", (e) => def.reject(e));
  req.end();
  await def;
  clearTimeout(timer);
  assertEquals(body, "HTTP/1.1");
});

Deno.test("[node/http] ClientRequest PATCH", async () => {
  let body = "";
  const def = deferred();
  const req = http.request("http://localhost:4545/echo_server", {
    method: "PATCH",
  }, (resp) => {
    resp.on("data", (chunk) => {
      body += chunk;
    });

    resp.on("end", () => {
      def.resolve();
    });
  });
  req.write("hello ");
  req.write("world");
  req.once("error", (e) => def.reject(e));
  req.end();
  await def;
  assertEquals(body, "hello world");
});

Deno.test("[node/http] ClientRequest PUT", async () => {
  let body = "";
  const def = deferred();
  const req = http.request("http://localhost:4545/echo_server", {
    method: "PUT",
  }, (resp) => {
    resp.on("data", (chunk) => {
      body += chunk;
    });

    resp.on("end", () => {
      def.resolve();
    });
  });
  req.write("hello ");
  req.write("world");
  req.once("error", (e) => def.reject(e));
  req.end();
  await def;
  assertEquals(body, "hello world");
});
