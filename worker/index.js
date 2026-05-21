const R2_PREFIX = "evm-compiler-bench";

const DATA_ROUTES = new Map([
  ["/latest.json", "latest"],
  ["/publish-manifest.json", "publish_manifest"],
  ["/report-model.json", "report_model"],
  ["/results.json", "results"],
  ["/run-manifest.json", "run_manifest"],
  ["/foundry-gas.jsonl", "foundry_gas"],
]);

function jsonResponse(value, init = {}) {
  const headers = new Headers(init.headers);
  headers.set("content-type", "application/json; charset=utf-8");
  headers.set("cache-control", init.cacheControl || "public, max-age=60");
  return new Response(JSON.stringify(value), { ...init, headers });
}

function notFound(message) {
  return jsonResponse({ error: message }, { status: 404, cacheControl: "no-store" });
}

async function latestManifest(env) {
  const object = await env.RESULTS.get(`${R2_PREFIX}/latest.json`);
  if (!object) return null;
  return object.json();
}

function objectResponse(object, cacheControl) {
  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("etag", object.httpEtag);
  headers.set("cache-control", cacheControl);
  headers.set("x-r2-key", object.key);
  return new Response(object.body, { headers });
}

async function serveArtifact(env, artifactName) {
  const latest = await latestManifest(env);
  if (!latest) return notFound("No published benchmark run found.");
  if (artifactName === "latest") {
    return jsonResponse(latest, { cacheControl: "public, max-age=60" });
  }

  const artifact = latest.artifacts?.[artifactName];
  if (!artifact?.key) return notFound(`No artifact named ${artifactName}.`);

  const object = await env.RESULTS.get(artifact.key);
  if (!object) return notFound(`Artifact missing from R2: ${artifact.key}`);

  const immutable = artifact.key.includes("/runs/");
  return objectResponse(
    object,
    immutable ? "public, max-age=31536000, immutable" : "public, max-age=60",
  );
}

async function serveRunObject(env, pathname) {
  const key = `${R2_PREFIX}${pathname}`;
  const object = await env.RESULTS.get(key);
  if (!object) return notFound(`No R2 object at ${key}.`);
  return objectResponse(object, "public, max-age=31536000, immutable");
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const route = DATA_ROUTES.get(url.pathname);
    if (route) return serveArtifact(env, route);
    if (url.pathname.startsWith("/runs/")) return serveRunObject(env, url.pathname);
    return env.ASSETS.fetch(request);
  },
};
