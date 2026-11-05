// Minimal API client for the Rust jackett-server (subset of /api/v2.0).
let apiKey = "";

export function setApiKey(key) {
  apiKey = key || "";
}

export function getApiKey() {
  return apiKey;
}

// Route a release download through the Jackett proxy endpoint, whose server
// side download flow (before + infohash blocks) turns detail-page links into
// magnets. Plain magnet links are returned unchanged.
export function downloadProxyUrl(trackerId, link, magnetUri) {
  if (magnetUri) return magnetUri;
  if (!link || !/^https?:/.test(link)) return link || "";
  const id = trackerId || "";
  if (!id) return link;
  const u = new URL(
    "/api/v2.0/indexers/" + encodeURIComponent(id) + "/results/torznab/download",
    location.origin,
  );
  u.searchParams.set("path", link);
  if (apiKey) u.searchParams.set("apikey", apiKey);
  return u.toString();
}

export async function api(path, params) {
  const u = new URL(path, location.origin);
  for (const [k, v] of Object.entries(params || {})) {
    if (v !== "" && v != null) u.searchParams.set(k, v);
  }
  if (apiKey) u.searchParams.set("apikey", apiKey);
  const r = await fetch(u);
  if (!r.ok) {
    throw new Error(r.status + " " + u.pathname + " — " + (await r.text()).slice(0, 200));
  }
  return r;
}

export async function apiJson(path, params) {
  return (await api(path, params)).json();
}

export function getServerConfig() {
  return apiJson("/api/v2.0/server/config");
}

export function updateServerConfig(settings) {
  return fetch("/api/v2.0/server/config", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  }).then(async (r) => {
    if (!r.ok) throw new Error(r.status + " " + (await r.text()).slice(0, 200));
    return r.json();
  });
}

export function getIndexers() {
  return apiJson("/api/v2.0/indexers");
}

export function getAvailableIndexers() {
  return apiJson("/api/v2.0/indexers/available");
}

export function getCache() {
  return apiJson("/api/v2.0/cache");
}

// C#-style per-indexer configuration schema (settings + current values + caps).
export function getIndexerConfig(id) {
  return apiJson("/api/v2.0/indexers/" + encodeURIComponent(id) + "/config");
}

export async function deleteIndexer(id) {
  const r = await fetch("/api/v2.0/indexers/" + encodeURIComponent(id), { method: "DELETE" });
  if (!r.ok) throw new Error(r.status + " " + (await r.text()).slice(0, 200));
}

export async function addIndexer(id, fields) {
  const r = await fetch("/api/v2.0/indexers", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id, fields }),
  });
  if (!r.ok) throw new Error(r.status + " " + (await r.text()).slice(0, 200));
}

// Returns the full response so callers can read both the release rows
// (`Results`) and the per-indexer status block (`Indexers`: Status / Error /
// Results). The latter is surfaced back into the Configured Indexers list so a
// failed/slow indexer is visible at a glance.
export async function manualSearch(id, q, cat) {
  return apiJson(
    "/api/v2.0/indexers/" + encodeURIComponent(id) + "/results/manualsearch",
    { q, cat },
  );
}

// Mirrors C#'s #jackett-test-all: POSTs to the indexer's real Test endpoint
// (empty search with is_test=true). Returns { ok, error, results } so callers
// can surface the outcome into the Status column.
export async function testIndexer(id, apiKey) {
  const u = new URL(
    "/api/v2.0/indexers/" + encodeURIComponent(id) + "/Test",
    location.origin,
  );
  if (apiKey) u.searchParams.set("apikey", apiKey);
  try {
    const r = await fetch(u, { method: "POST" });
    let data = {};
    try {
      data = await r.json();
    } catch {
      /* non-JSON error body */
    }
    if (r.ok && data.status === "OK") {
      return { ok: true, error: "", results: data.results || 0 };
    }
    return { ok: false, error: data.error || "HTTP " + r.status, results: 0 };
  } catch (e) {
    return { ok: false, error: e.message, results: 0 };
  }
}
