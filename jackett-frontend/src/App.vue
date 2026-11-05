<script setup>
import { ref, onMounted } from "vue";
import { api, getServerConfig, getIndexers, deleteIndexer, setApiKey, testIndexer } from "./api";
import AppHeader from "./components/AppHeader.vue";
import IndexerTable from "./components/IndexerTable.vue";
import ManualSearchModal from "./components/ManualSearchModal.vue";
import CacheModal from "./components/CacheModal.vue";
import AddIndexerModal from "./components/AddIndexerModal.vue";
import SettingsModal from "./components/SettingsModal.vue";
import ConfigModal from "./components/ConfigModal.vue";

const version = ref("");
const apiKey = ref("");
const healthOk = ref(null);
const indexers = ref([]);
const indexersError = ref("");
const activeModal = ref(null);
const searchTarget = ref("");
const configTarget = ref("");
const testingAll = ref(false);

async function loadConfig() {
  try {
    const cfg = await getServerConfig();
    setApiKey(cfg.api_key || "");
    apiKey.value = cfg.api_key || "";
    version.value = "v" + (cfg.app_version || "?");
  } catch {
    version.value = "config unavailable";
  }
}

async function checkHealth() {
  try {
    await api("/health").then((r) => r.text());
    healthOk.value = true;
  } catch {
    healthOk.value = false;
  }
}

async function loadIndexers() {
  try {
    indexers.value = await getIndexers();
    indexersError.value = "";
  } catch (e) {
    indexersError.value = e.message;
  }
}

async function removeIndexer(id) {
  if (!confirm("删除索引器 " + id + "？")) return;
  try {
    await deleteIndexer(id);
    await loadIndexers();
  } catch (e) {
    alert("删除失败：" + e.message);
  }
}

async function removeMany(ids) {
  if (!confirm("删除所选 " + ids.length + " 个索引器？")) return;
  const failed = [];
  for (const id of ids) {
    try {
      await deleteIndexer(id);
    } catch {
      failed.push(id);
    }
  }
  await loadIndexers();
  if (failed.length) alert("删除失败：" + failed.join(", "));
}

function onAdded() {
  loadIndexers();
}

// Toolbar opens the search modal unscoped; a row's Search button preselects
// that indexer in the tracker filter.
function openSearch(id) {
  searchTarget.value = id || "";
  activeModal.value = "search";
}

function openConfig(id) {
  configTarget.value = id || "";
  activeModal.value = "config";
}

function onConfigSaved() {
  activeModal.value = null;
  loadIndexers();
}

// Manual search reports each indexer's health (Status / Error / Results); mirror
// it onto the Configured Indexers list so a failing indexer is visible there.
function onSearchStatus(list) {
  for (const s of list) {
    const it = indexers.value.find((i) => i.id === s.id);
    if (it) {
      it.status = s.status;
      it.lastError = s.error || "";
      it.lastResults = s.results;
    }
  }
}

// A single indexer's Test button feeds the same Status column.
function onTestStatus(s) {
  const it = indexers.value.find((i) => i.id === s.id);
  if (it) {
    it.status = s.status;
    it.lastError = s.error || "";
  }
}

// "Test All" (mirror of C#'s #jackett-test-all): probe every configured indexer's
// caps endpoint and write the outcome into the Status column. Bounded
// concurrency so a large list doesn't hammer the server at once.
async function testAll() {
  if (testingAll.value) return;
  testingAll.value = true;
  const list = indexers.value;
  const CONC = 8;
  for (let start = 0; start < list.length; start += CONC) {
    const chunk = list.slice(start, start + CONC);
    await Promise.allSettled(
      chunk.map(async (i) => {
        const res = await testIndexer(i.id, apiKey.value);
        const it = indexers.value.find((x) => x.id === i.id);
        if (it) {
          it.status = res.ok ? "OK" : "Error";
          it.lastError = res.error || "";
        }
      }),
    );
  }
  testingAll.value = false;
}

onMounted(() => {
  loadConfig();
  checkHealth();
  loadIndexers();
});
</script>

<template>
  <AppHeader :version="version" :health-ok="healthOk" />

  <main>
    <nav class="toolbar">
      <button type="button" class="navbtn" @click="activeModal = 'add'">Add Indexer</button>
      <button type="button" class="navbtn" @click="openSearch()">Manual Search</button>
      <button type="button" class="navbtn" @click="activeModal = 'cache'">View cached releases</button>
      <button type="button" class="navbtn" @click="activeModal = 'settings'">Settings</button>
    </nav>

    <section class="card">
      <h2>Configured Indexers <span v-if="indexers.length">({{ indexers.length }})</span></h2>
      <div v-if="indexersError" class="empty">无法加载索引器：{{ indexersError }}</div>
      <div v-else-if="!indexers.length" class="empty">
        没有已配置的索引器。用 JACKETT_&lt;ID&gt;_* 环境变量启用，或在 Add Indexer 中勾选添加。
      </div>
      <IndexerTable
        v-else
        :indexers="indexers"
        :api-key="apiKey"
        :testing-all="testingAll"
        @delete="removeIndexer"
        @delete-many="removeMany"
        @search="openSearch"
        @edit="openConfig"
        @test-status="onTestStatus"
        @test-all="testAll"
      />
    </section>
  </main>

  <footer>
    这是 Rust 移植版内置的轻量界面（仅使用已移植的 API）。若要使用自己的静态页面，
    设置环境变量 <code>JACKETT_WWWROOT</code> 指向含 <code>index.html</code> 的目录。
    Torznab 地址示例：<code>/torznab/&lt;id&gt;/api?t=caps</code>。
  </footer>

  <ManualSearchModal
    v-if="activeModal === 'search'"
    :indexers="indexers"
    :initial-tracker="searchTarget"
    @close="activeModal = null"
    @update-status="onSearchStatus"
  />
  <CacheModal v-if="activeModal === 'cache'" @close="activeModal = null" />
  <AddIndexerModal v-if="activeModal === 'add'" @close="activeModal = null" @added="onAdded" />
  <SettingsModal v-if="activeModal === 'settings'" @close="activeModal = null" />
  <ConfigModal
    v-if="activeModal === 'config'"
    :indexer-id="configTarget"
    @close="activeModal = null"
    @saved="onConfigSaved"
  />
</template>
