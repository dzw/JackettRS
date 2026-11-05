<script setup>
import { ref, reactive, onMounted } from "vue";
import Modal from "./Modal.vue";
import { api, apiJson, getServerConfig, updateServerConfig } from "../api";
import { copyText } from "../clipboard";

// Mirror of the C# server settings screen: read-only API key with copy,
// then the full configuration form applied with one "Apply server settings".
// View logs / Check for updates are placeholders — the corresponding server
// APIs are not ported yet.
const emit = defineEmits(["close"]);

const loading = ref(true);
const loadError = ref("");
const saving = ref(false);
const statusMsg = ref("");
const statusErr = ref(false);

const apiKey = ref("");
const appVersion = ref("");
const form = reactive({
  password: "",
  basepathoverride: "",
  baseurloverride: "",
  port: "",
  blackholedir: "",
  proxy_type: "-1",
  proxy_url: "",
  proxy_port: "",
  proxy_username: "",
  proxy_password: "",
  external: false,
  local_bind_address: "",
  cors: false,
  updatedisabled: false,
  prerelease: false,
  logging: false,
  cache_enabled: false,
  cache_ttl: "",
  cache_max_results_per_indexer: "",
  flaresolverrurl: "",
  flaresolverr_maxtimeout: "",
  omdbkey: "",
  omdburl: "",
});

function opt(s) {
  return s == null ? "" : String(s);
}

onMounted(async () => {
  try {
    const cfg = await getServerConfig();
    apiKey.value = cfg.api_key || "";
    appVersion.value = cfg.app_version || "";
    form.port = cfg.port > 0 ? String(cfg.port) : "";
    form.basepathoverride = opt(cfg.basepathoverride);
    form.baseurloverride = opt(cfg.baseurloverride);
    form.blackholedir = opt(cfg.blackholedir);
    form.proxy_type = String(cfg.proxy_type ?? "-1");
    form.proxy_url = opt(cfg.proxy_url);
    form.proxy_port = cfg.proxy_port != null ? String(cfg.proxy_port) : "";
    form.proxy_username = opt(cfg.proxy_username);
    form.proxy_password = opt(cfg.proxy_password);
    form.external = !!cfg.external;
    form.local_bind_address = opt(cfg.local_bind_address) || "127.0.0.1";
    form.cors = !!cfg.cors;
    form.updatedisabled = !!cfg.updatedisabled;
    form.prerelease = !!cfg.prerelease;
    form.logging = !!cfg.logging;
    form.cache_enabled = !!cfg.cache_enabled;
    form.cache_ttl = cfg.cache_ttl > 0 ? String(cfg.cache_ttl) : "";
    form.cache_max_results_per_indexer =
      cfg.cache_max_results_per_indexer > 0 ? String(cfg.cache_max_results_per_indexer) : "";
    form.flaresolverrurl = opt(cfg.flaresolverrurl);
    form.flaresolverr_maxtimeout = cfg.flaresolverr_maxtimeout > 0 ? String(cfg.flaresolverr_maxtimeout) : "";
    form.omdbkey = opt(cfg.omdbkey);
    form.omdburl = opt(cfg.omdburl);
  } catch (e) {
    loadError.value = e.message;
  } finally {
    loading.value = false;
  }
});

async function copyApiKey() {
  if (await copyText(apiKey.value)) {
    statusMsg.value = "API Key 已复制到剪贴板。";
    statusErr.value = false;
  } else {
    statusMsg.value = "复制失败，请手动选择复制。";
    statusErr.value = true;
  }
}

function buildPayload() {
  const p = {};
  p.port = parseInt(form.port, 10) || 9117;
  p.password = form.password; // blank disables, matching C#
  p.basepathoverride = form.basepathoverride;
  p.baseurloverride = form.baseurloverride;
  p.blackholedir = form.blackholedir;
  p.proxy_type = parseInt(form.proxy_type, 10);
  p.proxy_url = form.proxy_url;
  p.proxy_port = form.proxy_port ? parseInt(form.proxy_port, 10) : null;
  p.proxy_username = form.proxy_username;
  p.proxy_password = form.proxy_password;
  p.external = form.external;
  p.local_bind_address = form.local_bind_address;
  p.cors = form.cors;
  p.updatedisabled = form.updatedisabled;
  p.prerelease = form.prerelease;
  p.logging = form.logging;
  p.cache_enabled = form.cache_enabled;
  p.cache_ttl = parseInt(form.cache_ttl, 10) || 0;
  p.cache_max_results_per_indexer = parseInt(form.cache_max_results_per_indexer, 10) || 0;
  p.flaresolverrurl = form.flaresolverrurl;
  p.flaresolverr_maxtimeout = parseInt(form.flaresolverr_maxtimeout, 10) || 0;
  p.omdbkey = form.omdbkey;
  p.omdburl = form.omdburl;
  return p;
}

async function applySettings() {
  saving.value = true;
  statusMsg.value = "正在保存设置…";
  statusErr.value = false;
  try {
    await updateServerConfig(buildPayload());
    statusMsg.value = "设置已保存。端口、绑定地址等需重启服务后生效。";
    if (form.password) {
      form.password = "";
    }
  } catch (e) {
    statusMsg.value = "保存失败：" + e.message;
    statusErr.value = true;
  } finally {
    saving.value = false;
  }
}

async function viewLogs() {
  try {
    const r = await api("/api/v2.0/server/logs");
    const text = await r.text();
    statusMsg.value = "最近日志：" + text.slice(-600);
    statusErr.value = false;
  } catch (e) {
    statusMsg.value = "获取日志失败：" + e.message;
    statusErr.value = true;
  }
}

function checkUpdates() {
  statusMsg.value = "Check for updates 尚未支持（updater API 未移植，可用 jackett-updater 命令）。";
  statusErr.value = true;
}
</script>

<template>
  <Modal title="Jackett Configuration" @close="$emit('close')">
    <div class="modal-body">
      <p v-if="loadError" class="empty">无法加载配置：{{ loadError }}</p>

      <template v-else>
        <div class="input-area">
          <span class="input-header">API Key: </span>
          <input class="form-control" type="password" :value="apiKey" readonly placeholder="API Key" />
          <button type="button" class="ibtn primary" title="Copy API Key to clipboard" @click="copyApiKey">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <rect x="9" y="9" width="12" height="12" rx="2" />
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
            </svg>
          </button>
        </div>

        <div class="settings-actions">
          <button type="button" class="navbtn primary-action" :disabled="saving" @click="applySettings">
            Apply server settings
          </button>
          <button type="button" class="navbtn" @click="viewLogs">View logs</button>
          <button type="button" class="navbtn" @click="checkUpdates">Check for updates</button>
        </div>

        <div class="input-area">
          <span class="input-header">Admin password: </span>
          <input v-model="form.password" class="form-control" type="password" placeholder="Blank to disable" />
        </div>
        <div class="input-area">
          <span class="input-header">Base path override: </span>
          <input v-model="form.basepathoverride" class="form-control" type="text" placeholder="/jackett" />
        </div>
        <div class="input-area">
          <span class="input-header">Base URL override: </span>
          <input v-model="form.baseurloverride" class="form-control" type="url" placeholder="http://jackett:9117" />
        </div>
        <div class="input-area">
          <span class="input-header">Server port: </span>
          <input v-model="form.port" class="form-control" type="text" placeholder="9117" />
        </div>
        <div class="input-area">
          <span class="input-header">Blackhole directory: </span>
          <input v-model="form.blackholedir" class="form-control" type="text" placeholder="c:\torrents\" />
        </div>

        <h2>Proxy</h2>
        <div class="input-area">
          <span class="input-header">Proxy type: </span>
          <select v-model="form.proxy_type" class="form-control">
            <option value="-1">Disabled</option>
            <option value="0">HTTP</option>
            <option value="1">SOCKS4</option>
            <option value="2">SOCKS5</option>
          </select>
        </div>
        <div v-if="form.proxy_type !== '-1'" class="proxy-warning">
          WARNING: The proxy option potentially leaks requests. Recommendation is to use a VPN.
        </div>
        <div class="input-area">
          <span class="input-header">Proxy URL: </span>
          <input v-model="form.proxy_url" class="form-control" type="text" />
        </div>
        <div class="input-area">
          <span class="input-header">Proxy port: </span>
          <input v-model="form.proxy_port" class="form-control" type="text" />
        </div>
        <div class="input-area">
          <span class="input-header">Proxy username: </span>
          <input v-model="form.proxy_username" class="form-control" type="text" />
        </div>
        <div class="input-area">
          <span class="input-header">Proxy password: </span>
          <input v-model="form.proxy_password" class="form-control" type="password" />
        </div>

        <h2>Server</h2>
        <label class="input-area check">
          <input v-model="form.external" type="checkbox" />
          <span class="input-header">External access</span>
        </label>
        <div class="input-area">
          <span class="input-header">Local bind address: </span>
          <input v-model="form.local_bind_address" class="form-control" type="text" placeholder="127.0.0.1" />
        </div>
        <label class="input-area check">
          <input v-model="form.cors" type="checkbox" />
          <span class="input-header">Allow CORS</span>
        </label>
        <label class="input-area check">
          <input v-model="form.updatedisabled" type="checkbox" />
          <span class="input-header">Disable auto update</span>
        </label>
        <label class="input-area check">
          <input v-model="form.prerelease" type="checkbox" />
          <span class="input-header">Update to pre-release</span>
        </label>
        <label class="input-area check">
          <input v-model="form.logging" type="checkbox" />
          <span class="input-header">Enhanced logging</span>
        </label>

        <h2>Cache</h2>
        <label class="input-area check">
          <input v-model="form.cache_enabled" type="checkbox" />
          <span class="input-header">Cache enabled (recommended)</span>
        </label>
        <div class="input-area">
          <span class="input-header">Cache TTL (seconds): </span>
          <input v-model="form.cache_ttl" class="form-control" type="text" placeholder="2100" />
        </div>
        <div class="input-area">
          <span class="input-header">Cache max results per indexer: </span>
          <input v-model="form.cache_max_results_per_indexer" class="form-control" type="text" placeholder="1000" />
        </div>

        <h2>Indexer enhancements</h2>
        <div class="input-area">
          <span class="input-header">FlareSolverr API URL: </span>
          <input v-model="form.flaresolverrurl" class="form-control" type="url" placeholder="Blank for default" />
        </div>
        <div class="input-area">
          <span class="input-header">FlareSolverr Max Timeout (ms): </span>
          <input v-model="form.flaresolverr_maxtimeout" class="form-control" type="text" placeholder="55000" />
        </div>
        <div class="input-area">
          <span class="input-header">OMDB API key: </span>
          <input v-model="form.omdbkey" class="form-control" type="text" />
        </div>
        <div class="input-area">
          <span class="input-header">OMDB API Url: </span>
          <input v-model="form.omdburl" class="form-control" type="text" placeholder="Blank for default" />
        </div>

        <p class="status" :class="{ err: statusErr }">{{ statusMsg }}</p>
        <div class="input-header" style="padding-top: 8px">Jackett Version {{ appVersion }}</div>
      </template>
    </div>
  </Modal>
</template>
