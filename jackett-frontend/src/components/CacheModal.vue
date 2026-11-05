<script setup>
import { ref, computed, onMounted } from "vue";
import Modal from "./Modal.vue";
import { getCache, downloadProxyUrl } from "../api";
import { fmtSize, fmtDate } from "../format";

// Mirror of the C# "Cached Releases" screen: 13 columns including
// Files/Grabs/Seeders/Leechers/Download- & UploadVolumeFactor/Download,
// with Tracker and Category filter dropdowns in the header's second row.
defineEmits(["close"]);

const items = ref([]);
const loadError = ref("");
const needle = ref("");
const trackerFilter = ref("");
const categoryFilter = ref("");

const trackerOptions = computed(() =>
  [...new Set(items.value.map((t) => t.tracker_id || t.tracker || "").filter(Boolean))].sort(),
);
const categoryOptions = computed(() => {
  const s = new Set();
  for (const t of items.value) {
    const c = categoryOf(t);
    if (c) s.add(c);
  }
  return [...s].sort();
});

function categoryOf(t) {
  if (t.category_desc) return t.category_desc;
  const c = (t.release || {}).category || [];
  return c.length ? c.join(", ") : "";
}

function dlLink(t) {
  const r = t.release || {};
  return downloadProxyUrl(t.tracker_id || t.tracker, r.link, r.magnet_uri);
}

const filtered = computed(() => {
  const q = needle.value.trim().toLowerCase();
  return items.value.filter((t) => {
    if (trackerFilter.value && (t.tracker_id || t.tracker || "") !== trackerFilter.value) return false;
    if (categoryFilter.value && categoryOf(t) !== categoryFilter.value) return false;
    if (!q) return true;
    const title = (t.release || {}).title || "";
    const label = t.tracker_id || t.tracker || "";
    return title.toLowerCase().includes(q) || label.toLowerCase().includes(q);
  });
});

function num(v) {
  return v == null ? "—" : v;
}

// Column sorting (mirrors the C# DataTables header behaviour): clicking a
// sortable header toggles asc/desc and the active column shows ▲/▼.
const sortKey = ref("");
const sortDir = ref("asc");
const SORTABLE = new Set([
  "published", "firstseen", "tracker", "title", "size", "category",
  "files", "grabs", "seeders", "peers", "dlf", "ulf",
]);
function rowVal(t, key) {
  const r = t.release || {};
  switch (key) {
    case "published": return Date.parse(r.publish_date) || 0;
    case "firstseen": return Date.parse(t.first_seen) || 0;
    case "tracker": return (t.tracker_id || t.tracker || "").toLowerCase();
    case "title": return (r.title || "").toLowerCase();
    case "size": return Number(r.size) || 0;
    case "category": return categoryOf(t).toLowerCase();
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
  const rows = filtered.value;
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

onMounted(async () => {
  try {
    items.value = await getCache();
  } catch (e) {
    loadError.value = e.message;
  }
});
</script>

<template>
  <Modal
    title="Cached Releases"
    description="This screen shows releases which have been recently returned from Jackett. Only the last 300 releases for each tracker are returned."
    @close="$emit('close')"
  >
    <div class="modal-body">
      <div class="modal-filters">
        <input v-model="needle" name="cache-search" placeholder="Search cached releases…" />
      </div>

      <p v-if="loadError" class="empty">无法加载缓存：{{ loadError }}</p>
      <div v-else class="table-wrap" style="padding-top: 12px">
        <table class="cache-table">
          <thead>
            <tr>
              <th :class="hdrCls('published')" @click="toggleSort('published')">Published</th>
              <th :class="hdrCls('firstseen')" @click="toggleSort('firstseen')">First Seen</th>
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
            <tr class="filter-row">
              <th></th>
              <th></th>
              <th>
                <select v-model="trackerFilter">
                  <option value=""></option>
                  <option v-for="t in trackerOptions" :key="t" :value="t">{{ t }}</option>
                </select>
              </th>
              <th></th>
              <th></th>
              <th></th>
              <th>
                <select v-model="categoryFilter">
                  <option value=""></option>
                  <option v-for="c in categoryOptions" :key="c" :value="c">{{ c }}</option>
                </select>
              </th>
              <th></th>
              <th></th>
              <th></th>
              <th></th>
              <th></th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="!filtered.length">
              <td colspan="13" class="empty">尚无缓存。先做一次搜索。</td>
            </tr>
            <tr v-for="(t, idx) in sortedRows" :key="idx">
              <td>{{ fmtDate((t.release || {}).publish_date) }}</td>
              <td>{{ fmtDate(t.first_seen) }}</td>
              <td>{{ t.tracker_id || t.tracker || "" }}</td>
              <td>{{ (t.release || {}).title || "(无标题)" }}</td>
              <td class="num">{{ fmtSize((t.release || {}).size) }}</td>
              <td class="num">{{ num((t.release || {}).files) }}</td>
              <td>{{ categoryOf(t) }}</td>
              <td class="num">{{ num((t.release || {}).grabs) }}</td>
              <td class="num">{{ num((t.release || {}).seeders) }}</td>
              <td class="num">{{ num((t.release || {}).peers) }}</td>
              <td class="num">{{ num((t.release || {}).download_volume_factor) }}</td>
              <td class="num">{{ num((t.release || {}).upload_volume_factor) }}</td>
              <td class="fit">
                <a v-if="dlLink(t)" :href="dlLink(t)" class="dl-icon" title="Download">
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
    </div>
  </Modal>
</template>
