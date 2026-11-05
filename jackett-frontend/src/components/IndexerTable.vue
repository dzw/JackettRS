<script setup>
import { reactive, computed, ref } from "vue";
import { copyText } from "../clipboard";
import { testIndexer } from "../api";

// Mirror of the C# dashboard "Configured Indexers" table: one row per indexer
// with a site link + type label, a Status column, and an actions column (copy
// feeds / search / delete / test). Rows are selectable: a plain click selects
// just that row, Ctrl/Cmd+click toggles a single row, and Shift+click selects
// the contiguous range from the last anchor — the same list-view convention.
// The checkbox column is the equivalent batch-delete selector.
const props = defineProps({
  indexers: { type: Array, default: () => [] },
  apiKey: { type: String, default: "" },
  testingAll: { type: Boolean, default: false },
});
const emit = defineEmits(["delete", "delete-many", "search", "edit", "test-status", "test-all"]);

const copied = reactive({}); // `${id}:${kind}` -> true for ~1.4s after copy
const testState = reactive({}); // id -> "pending" | "ok" | "err"
const selected = reactive({}); // id -> true (batch delete)

const selectedIds = computed(() => Object.keys(selected));
const allSelected = computed(
  () => props.indexers.length > 0 && props.indexers.every((i) => selected[i.id]),
);

function toggleAll(checked) {
  for (const i of props.indexers) {
    if (checked) selected[i.id] = true;
    else delete selected[i.id];
  }
}

function toggleRow(i, checked) {
  if (checked) selected[i.id] = true;
  else delete selected[i.id];
}

// Anchor (index into the currently displayed/sorted rows) used by Shift+click
// range selection. Plain and Ctrl/Cmd clicks update it; a Shift click extends
// from it without moving it, so repeated Shift clicks grow the same range.
const lastAnchor = ref(-1);

function clearSelection() {
  for (const id of Object.keys(selected)) delete selected[id];
}

function rowClick(i, ev) {
  // Let the checkbox, action buttons and the site link handle their own clicks.
  if (ev.target.closest && ev.target.closest("input, button, a")) return;
  const list = sortedIndexers.value;
  const idx = list.findIndex((x) => x.id === i.id);
  if (idx < 0) return;

  if (ev.shiftKey && lastAnchor.value >= 0) {
    const [a, b] = idx < lastAnchor.value ? [idx, lastAnchor.value] : [lastAnchor.value, idx];
    for (let k = a; k <= b; k++) selected[list[k].id] = true;
    return;
  }
  if (ev.ctrlKey || ev.metaKey) {
    if (selected[i.id]) delete selected[i.id];
    else selected[i.id] = true;
    lastAnchor.value = idx;
    return;
  }
  // Plain click: single-select this row (and set the anchor).
  clearSelection();
  selected[i.id] = true;
  lastAnchor.value = idx;
}

function deleteSelected() {
  if (!selectedIds.value.length) return;
  emit("delete-many", selectedIds.value.slice());
  for (const id of selectedIds.value) delete selected[id];
}

function feedBase(id) {
  return location.origin + "/api/v2.0/indexers/" + encodeURIComponent(id) + "/results";
}

function rssUrl(id) {
  const u = new URL(feedBase(id) + "/torznab/api");
  if (props.apiKey) u.searchParams.set("apikey", props.apiKey);
  u.searchParams.set("t", "search");
  u.searchParams.set("cat", "");
  u.searchParams.set("q", "");
  return u.toString();
}

function torznabUrl(id) {
  return feedBase(id) + "/torznab/";
}

function potatoUrl(id) {
  return feedBase(id) + "/potato/";
}

async function copy(kind, id, text) {
  if (await copyText(text)) {
    copied[id + ":" + kind] = true;
    setTimeout(() => {
      delete copied[id + ":" + kind];
    }, 1400);
  }
}

// Sortable headers: clicking "Indexer" or "Status" reorders the rows. Status
// ranks Error (0) < OK (1) < untested (2) so a first asc click surfaces the
// broken indexers first.
const sortKey = ref("");
const sortDir = ref("asc");
function statusRank(i) {
  if (i.status === "Error") return 0;
  if (i.status === "OK") return 1;
  return 2;
}
function toggleSort(key) {
  if (sortKey.value === key) {
    sortDir.value = sortDir.value === "asc" ? "desc" : "asc";
  } else {
    sortKey.value = key;
    sortDir.value = "asc";
  }
}
function hdrCls(key) {
  return {
    sortable: true,
    "sort-asc": sortKey.value === key && sortDir.value === "asc",
    "sort-desc": sortKey.value === key && sortDir.value === "desc",
  };
}
const sortedIndexers = computed(() => {
  if (!sortKey.value) return props.indexers;
  const dir = sortDir.value === "asc" ? 1 : -1;
  return [...props.indexers].sort((a, b) => {
    let va, vb;
    if (sortKey.value === "status") {
      va = statusRank(a);
      vb = statusRank(b);
    } else {
      va = (a.name || a.id).toLowerCase();
      vb = (b.name || b.id).toLowerCase();
    }
    if (va < vb) return -dir;
    if (va > vb) return dir;
    return 0;
  });
});

async function test(i) {
  const id = i.id;
  testState[id] = "pending";
  const res = await testIndexer(id, props.apiKey);
  testState[id] = res.ok ? "ok" : "err";
  emit("test-status", { id, status: res.ok ? "OK" : "Error", error: res.error });
}

function testLabel(id) {
  if (testState[id] === "pending") return "Test…";
  if (testState[id] === "ok") return "Test ✓";
  if (testState[id] === "err") return "Test ✗";
  return "Test";
}
</script>

<template>
  <div class="table-wrap">
    <table class="indexer-table">
      <thead>
        <tr>
          <th class="checkboxColumn">
            <input
              type="checkbox"
              :checked="allSelected"
              :disabled="!indexers.length"
              aria-label="全选"
              @change="toggleAll($event.target.checked)"
            />
          </th>
          <th :class="hdrCls('name')" @click="toggleSort('name')">Indexer</th>
          <th class="fit" :class="hdrCls('status')" @click="toggleSort('status')">
            Status
            <button
              type="button"
              class="ibtn test-all"
              :disabled="testingAll || !indexers.length"
              :title="testingAll ? '正在测试所有索引器…' : '测试全部已配置索引器'"
              @click.stop="emit('test-all')"
            >
              {{ testingAll ? "Testing…" : "Test All" }}
            </button>
          </th>
          <th class="fit">
            <div class="head-actions">
              Actions
              <button
                type="button"
                class="ibtn danger"
                :disabled="!selectedIds.length"
                title="删除所选索引器"
                @click="deleteSelected"
              >
                删除所选{{ selectedIds.length ? " (" + selectedIds.length + ")" : "" }}
              </button>
            </div>
          </th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="i in sortedIndexers"
          :key="i.id"
          :class="{ 'selected-row': !!selected[i.id] }"
          @click="rowClick(i, $event)"
        >
          <td class="checkboxColumn">
            <input
              type="checkbox"
              :checked="!!selected[i.id]"
              :aria-label="'选择 ' + (i.name || i.id)"
              @change="toggleRow(i, $event.target.checked)"
            />
          </td>
          <td>
            <a target="_blank" rel="noopener" :href="i.site_link" :title="i.description">
              {{ i.name || i.id }}
            </a>
            <span class="type-label" :title="i.type">{{ i.type }}</span>
          </td>
          <td class="fit">
            <span
              v-if="i.status === 'Error'"
              class="status-badge err"
              :title="i.lastError"
            >Error</span>
            <span
              v-else-if="i.status === 'OK'"
              class="status-badge ok"
              :title="i.lastResults + ' results'"
            >OK{{ i.lastResults ? ' (' + i.lastResults + ')' : '' }}</span>
            <span v-else class="status-badge none" title="尚未搜索">—</span>
          </td>
          <td class="fit">
            <div class="indexer-buttons">
              <button
                type="button"
                class="ibtn info"
                :title="rssUrl(i.id)"
                @click="copy('rss', i.id, rssUrl(i.id))"
              >
                {{ copied[i.id + ':rss'] ? 'Copied!' : 'Copy RSS Feed' }}
              </button>
              <button
                type="button"
                class="ibtn info"
                :title="torznabUrl(i.id)"
                @click="copy('torznab', i.id, torznabUrl(i.id))"
              >
                {{ copied[i.id + ':torznab'] ? 'Copied!' : 'Copy Torznab Feed' }}
              </button>
              <button
                type="button"
                class="ibtn info"
                :title="potatoUrl(i.id)"
                @click="copy('potato', i.id, potatoUrl(i.id))"
              >
                {{ copied[i.id + ':potato'] ? 'Copied!' : 'Copy Potato Feed' }}
              </button>

              <button
                type="button"
                class="ibtn info"
                title="编辑配置"
                @click="emit('edit', i.id)"
              >
                编辑
              </button>
              <button
                type="button"
                class="ibtn success"
                title="Search"
                @click="emit('search', i.id)"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true">
                  <circle cx="10.5" cy="10.5" r="6.5" />
                  <line x1="15.5" y1="15.5" x2="21" y2="21" />
                </svg>
              </button>
              <button
                type="button"
                class="ibtn danger"
                title="Delete"
                @click="emit('delete', i.id)"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
                  <path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />
                </svg>
              </button>
              <button
                type="button"
                class="ibtn warning"
                :title="testState[i.id] === 'err' ? i.last_error || 'Test failed' : 'Test'"
                @click="test(i)"
              >
                {{ testLabel(i.id) }}
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
