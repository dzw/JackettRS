<script setup>
import { ref, computed, reactive, onMounted } from "vue";
import Modal from "./Modal.vue";
import { getAvailableIndexers, addIndexer } from "../api";

// Mirror of the C# "Select an indexer to setup" dialog: unconfigured
// definitions in a table with per-column filters (Categories / Type /
// Language), a checkbox + Add button for public trackers, and a Configure
// button for the private ones. Definitions with a persisted user config are
// hidden (C# unconfigured-indexer-datatable semantics).
const emit = defineEmits(["close", "added"]);

const available = ref([]);
const loadError = ref("");
const needle = ref("");
const catFilter = ref("");
const typeFilter = ref("");
const langFilter = ref("");
const statusMsg = ref("");
const adding = ref(false);

// id -> true for batch-selected rows (public trackers only).
const selected = reactive({});

const unconfigured = computed(() => available.value.filter((a) => !a.configured));

const categoryOptions = computed(() => {
  const s = new Set();
  for (const a of unconfigured.value) for (const c of a.categories || []) s.add(c);
  return [...s].sort();
});
const typeOptions = computed(() => {
  const s = new Set();
  for (const a of unconfigured.value) if (a.type) s.add(a.type);
  return ["public", "private", "semi-private"].filter((t) => s.has(t));
});
const languageOptions = computed(() => {
  const s = new Set();
  for (const a of unconfigured.value) if (a.language) s.add(a.language);
  return [...s].sort();
});

const filtered = computed(() => {
  const q = needle.value.trim().toLowerCase();
  const rows = unconfigured.value.filter((a) => {
    if (catFilter.value && !(a.categories || []).includes(catFilter.value)) return false;
    if (typeFilter.value && a.type !== typeFilter.value) return false;
    if (langFilter.value && a.language !== langFilter.value) return false;
    if (!q) return true;
    return (a.name || "").toLowerCase().includes(q) || (a.id || "").toLowerCase().includes(q);
  });
  return rows.sort((a, b) => (a.name || "").localeCompare(b.name || ""));
});

const selectedIds = computed(() => Object.keys(selected));
const isPublic = (a) => a.type === "public";

// Public rows visible under the current filters.
const selectableRows = computed(() => filtered.value.filter(isPublic));
const allSelected = computed(
  () => selectableRows.value.length > 0 && selectableRows.value.every((a) => selected[a.id]),
);

function toggle(a, checked) {
  if (checked) selected[a.id] = true;
  else delete selected[a.id];
}

function toggleAll(checked) {
  for (const a of selectableRows.value) {
    if (checked) selected[a.id] = true;
    else delete selected[a.id];
  }
}

async function persist(id) {
  await addIndexer(id, {});
  delete selected[id];
}

async function addOne(a) {
  statusMsg.value = "正在添加 " + a.name + "…";
  try {
    await persist(a.id);
    statusMsg.value = "已添加 " + a.name + "。";
    emit("added");
  } catch (e) {
    statusMsg.value = a.id + " 添加失败：" + e.message;
  }
}

async function addSelected() {
  if (!selectedIds.value.length) {
    statusMsg.value = "请先勾选要添加的索引器。";
    return;
  }
  adding.value = true;
  statusMsg.value = "添加中…";
  let ok = 0;
  const failed = [];
  for (const id of selectedIds.value) {
    try {
      await persist(id);
      ok++;
    } catch (e) {
      failed.push(id + ": " + e.message);
    }
  }
  statusMsg.value =
    "已添加 " + ok + (failed.length ? "，失败 " + failed.length + "：" + failed.join("; ") : "") + " 个索引器。";
  adding.value = false;
  if (ok) emit("added");
}

function configure(a) {
  statusMsg.value = "Configure 尚未支持：" + a.name + " 的配置对话框需后端 settings 读取 API（后续移植）。";
}

onMounted(async () => {
  try {
    available.value = await getAvailableIndexers();
  } catch (e) {
    loadError.value = e.message;
  }
});
</script>

<template>
  <Modal
    title="Select an indexer to setup"
    description="勾选或点 Add 添加公开索引器；私有索引器需要 Configure（暂未支持）。"
    @close="$emit('close')"
  >
    <div class="modal-body">
      <div class="modal-filters">
        <input v-model="needle" name="add-search" placeholder="Search indexers…" />
      </div>

      <p v-if="loadError" class="empty">无法加载可用索引器：{{ loadError }}</p>

      <div v-else class="table-wrap" style="padding-top: 12px">
        <table class="add-table">
          <thead>
            <tr>
              <th class="fit"></th>
              <th>Indexer</th>
              <th>Categories</th>
              <th class="fit">Type</th>
              <th class="fit">Language</th>
              <th class="fit"></th>
            </tr>
            <tr class="filter-row">
              <th>
                <input
                  type="checkbox"
                  :checked="allSelected"
                  :disabled="!selectableRows.length"
                  aria-label="全选"
                  title="全选当前列表中的公开索引器"
                  @change="toggleAll($event.target.checked)"
                />
              </th>
              <th></th>
              <th>
                <select v-model="catFilter">
                  <option value="">Show all</option>
                  <option v-for="c in categoryOptions" :key="c" :value="c">{{ c }}</option>
                </select>
              </th>
              <th>
                <select v-model="typeFilter">
                  <option value="">Show all</option>
                  <option v-for="t in typeOptions" :key="t" :value="t">{{ t }}</option>
                </select>
              </th>
              <th>
                <select v-model="langFilter">
                  <option value="">Show all</option>
                  <option v-for="l in languageOptions" :key="l" :value="l">{{ l }}</option>
                </select>
              </th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="!filtered.length">
              <td colspan="6" class="empty">没有匹配的未配置索引器。</td>
            </tr>
            <tr v-for="a in filtered" :key="a.id">
              <td class="checkboxColumn">
                <input
                  v-if="isPublic(a)"
                  type="checkbox"
                  :checked="!!selected[a.id]"
                  :aria-label="'选择 ' + a.name"
                  @change="toggle(a, $event.target.checked)"
                />
              </td>
              <td>
                <a target="_blank" rel="noopener" :href="a.site_link" :title="a.description">
                  {{ a.name }}
                </a>
              </td>
              <td>{{ (a.categories || []).join(", ") }}</td>
              <td class="fit">
                <span v-if="a.type" class="type-label" :class="a.type === 'public' ? '' : a.type === 'semi-private' ? 'warning' : 'danger'" :title="a.type">
                  {{ a.type }}
                </span>
              </td>
              <td class="fit">{{ a.language }}</td>
              <td class="fit">
                <div class="indexer-buttons">
                  <button
                    type="button"
                    class="ibtn primary"
                    :title="'Configure ' + a.name"
                    @click="configure(a)"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true">
                      <path d="M14.7 6.3a4.5 4.5 0 0 0-6 5.6L3 17.6V21h3.4l5.7-5.7a4.5 4.5 0 0 0 5.6-6l-3 3-2.4-.6-.6-2.4 3-3z" />
                    </svg>
                  </button>
                  <button
                    v-if="isPublic(a)"
                    type="button"
                    class="ibtn success"
                    :title="'Add ' + a.name"
                    @click="addOne(a)"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" aria-hidden="true">
                      <line x1="12" y1="5" x2="12" y2="19" />
                      <line x1="5" y1="12" x2="19" y2="12" />
                    </svg>
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div style="display: flex; align-items: center; gap: 10px; padding-top: 14px">
        <button type="button" :disabled="adding || !selectedIds.length" @click="addSelected">
          添加所选{{ selectedIds.length ? " (" + selectedIds.length + ")" : "" }}
        </button>
        <span class="add-msg">{{ statusMsg }}</span>
      </div>
    </div>
  </Modal>
</template>
