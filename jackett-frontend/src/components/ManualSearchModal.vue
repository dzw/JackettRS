<script setup>
import { ref, computed, watch } from "vue";
import Modal from "./Modal.vue";
import { manualSearch, downloadProxyUrl } from "../api";
import { fmtSize, fmtDate } from "../format";
import { categoryNames } from "../torznab_cats";

const props = defineProps({
  indexers: { type: Array, default: () => [] },
  initialTracker: { type: String, default: "" },
});
const emit = defineEmits(["close", "update-status"]);

const query = ref("");
const trackerId = ref(props.initialTracker);
const cat = ref("");
const searching = ref(false);
const statusMsg = ref("");
const statusErr = ref(false);
const results = ref([]);
// Per-indexer outcome of the last search, emitted up so the Configured Indexers
// table can show each indexer's health (Status / Error / Results).
const statuses = ref([]);
// Live progress: indexers still in flight, plus per-indexer errors as they come.
const pending = ref(0);
const totalTargets = ref(0);
const errorList = ref([]);

// Column sorting (mirrors the C# DataTables header behaviour): clicking a
// sortable header toggles asc/desc and the active column shows ▲/▼.
const sortKey = ref("seeders");
const sortDir = ref("desc");
const SORTABLE = new Set([
  "published", "tracker", "title", "size", "category",
  "files", "grabs", "seeders", "peers", "dlf", "ulf",
]);
function rowVal(row, key) {
  const r = row.release || {};
  switch (key) {
    case "published": return Date.parse(r.publish_date) || 0;
    case "tracker": return (row.tracker || row.tracker_id || "").toLowerCase();
    case "title": return (r.title || "").toLowerCase();
    case "size": return Number(r.size) || 0;
    case "category": return categoryNames(r).toLowerCase();
    case "files": return Number(r.files) || 0;
    case "grabs": return Number(r.grabs) || 0;
    case "seeders": return Number(r.seeders) || 0;
    case "peers": return Number(r.peers) || 0;
    case "dlf": return Number(r.download_volume_factor) || 0;
    case "ulf": return Number(r.upload_volume_factor) || 0;
    default: return 0;
  }
}
function toggleSort(key) {
  if (!SORTABLE.has(key)) return;
  if (sortKey.value === key) {
    sortDir.value = sortDir.value === "asc" ? "desc" : "asc";
  } else {
    sortKey.value = key;
    sortDir.value = "desc";
  }
}
function hdrCls(key) {
  return {
    sortable: SORTABLE.has(key),
    "sort-asc": sortKey.value === key && sortDir.value === "asc",
    "sort-desc": sortKey.value === key && sortDir.value === "desc",
  };
}
const sortedRows = computed(() => {
  const rows = filteredRows.value;
  if (!sortKey.value) return rows;
  const dir = sortDir.value === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const va = rowVal(a, sortKey.value);
    const vb = rowVal(b, sortKey.value);
    if (va < vb) return -dir;
    if (va > vb) return dir;
    return 0;
  });
});

// Result table client-side controls (mirror of the C# DataTables wrapper):
// keyword filter with -negative / a|b|c OR semantics, dead-torrent filter,
// page size and paging.
const resultFilter = ref("");
const showDead = ref(true);
const pageSize = ref(50);
const page = ref(1);

// Tracker filter lists every loaded indexer — a row's Search button must be
// able to preselect it even when the indexer is not yet configured (the
// search will then surface the configuration error instead of hiding it).
const searchTargets = computed(() => props.indexers);

function bySeeders(a, b) {
  return ((b.release || {}).seeders || 0) - ((a.release || {}).seeders || 0);
}

// A filter group matches when every keyword matches the title; keywords
// starting with "-" are negative matches. "|" separates OR alternatives.
function titleMatches(title, filter) {
  const t = title.toLowerCase();
  const groups = filter
    .split("|")
    .map((g) => g.trim())
    .filter(Boolean);
  if (!groups.length) return true;
  return groups.some((group) =>
    group
      .split(/\s+/)
      .filter(Boolean)
      .every((kw) => {
        const negative = kw.startsWith("-");
        const word = kw.replace(/^-+/, "").toLowerCase();
        if (!word) return true;
        const has = t.includes(word);
        return negative ? !has : has;
      }),
  );
}

const filteredRows = computed(() => {
  const filter = resultFilter.value.trim();
  const dead = showDead.value;
  const rows = results.value.filter((row) => {
    const r = row.release || {};
    if (!dead && (r.seeders === 0)) return false;
    if (filter && !titleMatches(r.title || "", filter)) return false;
    return true;
  });
  return rows.sort(bySeeders);
});

const pageSizeNum = computed(() =>
  pageSize.value === -1 ? Infinity : pageSize.value,
);
const totalPages = computed(() =>
  Math.max(1, Math.ceil(sortedRows.value.length / pageSizeNum.value)),
);
const pagedRows = computed(() => {
  const size = pageSizeNum.value;
  if (!isFinite(size)) return sortedRows.value;
  const start = (page.value - 1) * size;
  return sortedRows.value.slice(start, start + size);
});
const rangeText = computed(() => {
  const total = sortedRows.value.length;
  if (!total) return "Showing 0 to 0 of 0 entries";
  const size = pageSizeNum.value;
  if (!isFinite(size)) return `Showing 1 to ${total} of ${total} entries`;
  const start = (page.value - 1) * size + 1;
  const end = Math.min(start + size - 1, total);
  return `Showing ${start} to ${end} of ${total} entries`;
});

watch([resultFilter, showDead, pageSize], () => {
  page.value = 1;
});
watch([sortKey, sortDir], () => {
  page.value = 1;
});
watch(filteredRows, (rows) => {
  // Streaming results can shrink the current page under us.
  if (page.value > totalPages.value) page.value = totalPages.value;
});

function dlHref(row) {
  const r = row.release || {};
  return downloadProxyUrl(row.tracker_id || row.tracker, r.link, r.magnet_uri);
}

// Results stream in per indexer: each settled request appends its rows and
// re-sorts, so the table fills up as responses arrive (like the C# UI).
// The special "all" target uses the server-side aggregate endpoint, which
// needs no per-indexer configuration (covers every public tracker) — the
// desktop shell relies on it for out-of-the-box searching.
async function doSearch() {
  const q = query.value.trim();
  const c = cat.value.trim();
  const configuredIds = searchTargets.value
    .filter((i) => i.configured !== false)
    .map((i) => i.id);
  const ids = trackerId.value === "all"
    ? ["all"]
    : trackerId.value
      ? [trackerId.value]
      : configuredIds.length
        ? configuredIds
        : ["all"]; // no configured indexers → fall back to the aggregate
  if (!ids.length) {
    statusMsg.value = "没有可用的索引器。请先在 Add Indexer 中添加。";
    statusErr.value = true;
    return;
  }
  searching.value = true;
  results.value = [];
  errorList.value = [];
  statuses.value = [];
  pending.value = ids.length;
  totalTargets.value = ids.length;
  statusMsg.value = `正在搜索 ${ids.length} 个索引器…`;
  statusErr.value = false;

  const settle = async (id) => {
    try {
      const data = await manualSearch(id, q, c);
      const rows = data.Results || data.results || [];
      if (rows.length) {
        results.value.push(...rows);
        results.value.sort(bySeeders);
      }
      const st = (data.Indexers || [])[0];
      if (st) {
        statuses.value.push({
          id,
          status: st.Status,
          error: st.Error || "",
          results: st.Results ?? 0,
        });
      }
    } catch (e) {
      errorList.value.push(id + ": " + e.message);
      statuses.value.push({ id, status: "Error", error: e.message, results: 0 });
    } finally {
      pending.value -= 1;
      if (pending.value === 0) {
        searching.value = false;
        const n = results.value.length;
        statusMsg.value = errorList.value.length
          ? `完成，${n} 条结果；部分失败：${errorList.value.join("; ")}`
          : `完成，${n} 条结果。`;
        statusErr.value = errorList.value.length > 0;
        emit("update-status", statuses.value);
      } else {
        statusMsg.value = `正在搜索 ${ids.length} 个索引器… 已返回 ${totalTargets.value - pending.value}（失败 ${errorList.value.length}）`;
      }
    }
  };

  await Promise.allSettled(ids.map((id) => settle(id)));
}
</script>

<template>
  <Modal
    title="Manual search"
    description="You can search all configured indexers from this screen."
    @close="$emit('close')"
  >
    <div class="modal-body">
      <form @submit.prevent="doSearch">
        <input id="query" v-model="query" placeholder="Query" />
        <select id="filter" v-model="trackerId">
          <option value="all">all（聚合全部公开源，无需配置）</option>
          <option value="">all（已配置索引器）</option>
          <option v-for="i in searchTargets" :key="i.id" :value="i.id">
            {{ i.name || i.id }}
          </option>
        </select>
        <input id="cat" v-model="cat" placeholder="Filter (category id, optional)" />
        <button type="submit" :disabled="searching">Search</button>
      </form>

      <p class="status" :class="{ err: statusErr }">{{ statusMsg }}</p>

      <h2>
        Results
        <span v-if="filteredRows.length">({{ filteredRows.length }})</span>
      </h2>

      <div class="result-toolbar">
        <label>
          Show
          <select v-model.number="pageSize">
            <option :value="10">10</option>
            <option :value="20">20</option>
            <option :value="50">50</option>
            <option :value="100">100</option>
            <option :value="250">250</option>
            <option :value="500">500</option>
            <option :value="-1">All</option>
          </select>
          entries
        </label>
        <label class="grow">
          Search:
          <input
            v-model="resultFilter"
            type="search"
            title="Keyword starting with &quot;-&quot; is a negative match. Keywords separated by &quot;|&quot; are OR filters."
            placeholder="过滤结果（- 排除，| OR）"
          />
        </label>
        <label class="nowrap">
          <input v-model="showDead" type="checkbox" /> Show dead torrents
        </label>
      </div>

      <div class="table-wrap">
        <table class="cache-table">
          <thead>
            <tr>
              <th :class="hdrCls('published')" @click="toggleSort('published')">Published</th>
              <th :class="hdrCls('tracker')" @click="toggleSort('tracker')">Tracker</th>
              <th :class="hdrCls('title')" @click="toggleSort('title')">Name</th>
              <th class="num" :class="hdrCls('size')" @click="toggleSort('size')">Size</th>
              <th class="num" :class="hdrCls('files')" @click="toggleSort('files')" title="Files">F</th>
              <th :class="hdrCls('category')" @click="toggleSort('category')">Category</th>
              <th class="num" :class="hdrCls('grabs')" @click="toggleSort('grabs')" title="Grabs">G</th>
              <th class="num" :class="hdrCls('seeders')" @click="toggleSort('seeders')" title="Seeders">S</th>
              <th class="num" :class="hdrCls('peers')" @click="toggleSort('peers')" title="Leechers">L</th>
              <th class="num fit" :class="hdrCls('dlf')" @click="toggleSort('dlf')" title="DownloadVolumeFactor">DLF</th>
              <th class="num fit" :class="hdrCls('ulf')" @click="toggleSort('ulf')" title="UploadVolumeFactor">ULF</th>
              <th class="fit" :class="hdrCls('dl')" @click="toggleSort('dl')" title="Download">DL</th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="!pagedRows.length">
              <td colspan="12" class="empty">
                {{ searching ? `搜索中（${pending} 个索引器未返回）…` : "没有找到结果。" }}
              </td>
            </tr>
            <tr v-for="(row, idx) in pagedRows" :key="idx">
              <td>{{ fmtDate((row.release || {}).publish_date) }}</td>
              <td>{{ row.tracker || row.tracker_id || "" }}</td>
              <td>{{ (row.release || {}).title || "(无标题)" }}</td>
              <td class="num">{{ fmtSize((row.release || {}).size) }}</td>
              <td class="num">{{ (row.release || {}).files == null ? "—" : (row.release || {}).files }}</td>
              <td>{{ categoryNames(row.release || {}) }}</td>
              <td class="num">{{ (row.release || {}).grabs == null ? "—" : (row.release || {}).grabs }}</td>
              <td class="num">{{ (row.release || {}).seeders == null ? "—" : (row.release || {}).seeders }}</td>
              <td class="num">{{ (row.release || {}).peers == null ? "—" : (row.release || {}).peers }}</td>
              <td class="num">{{ (row.release || {}).download_volume_factor == null ? "—" : (row.release || {}).download_volume_factor }}</td>
              <td class="num">{{ (row.release || {}).upload_volume_factor == null ? "—" : (row.release || {}).upload_volume_factor }}</td>
              <td class="fit">
                <a v-if="dlHref(row)" :href="dlHref(row)" class="dl-icon" title="Download">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
                    <path d="M12 3v12M6 11l6 6 6-6M4 21h16" />
                  </svg>
                </a>
                <span v-else class="dim">—</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div class="result-pager">
        <span class="dim">{{ rangeText }}</span>
        <button
          type="button"
          class="navbtn"
          :disabled="page <= 1"
          @click="page -= 1"
        >
          Previous
        </button>
        <span class="dim">{{ page }} / {{ totalPages }}</span>
        <button
          type="button"
          class="navbtn"
          :disabled="page >= totalPages"
          @click="page += 1"
        >
          Next
        </button>
      </div>
    </div>
  </Modal>
</template>
