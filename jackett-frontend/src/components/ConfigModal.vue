<script setup>
import { ref, reactive, onMounted, computed } from "vue";
import { getIndexerConfig, addIndexer } from "../api";

const props = defineProps({
  indexerId: { type: String, required: true },
});
const emit = defineEmits(["close", "saved"]);

const cfg = ref(null);
const loading = ref(true);
const error = ref("");
const saving = ref(false);
const saveError = ref("");
const values = reactive({}); // setting id -> current string value

const titleLink = computed(() => (cfg.value && cfg.value.link ? cfg.value.link : ""));
const hostLabel = computed(() => {
  if (!titleLink.value) return "";
  try {
    return new URL(titleLink.value).host;
  } catch {
    return titleLink.value;
  }
});

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const data = await getIndexerConfig(props.indexerId);
    cfg.value = data;
    for (const s of data.settings || []) {
      values[s.id] = s.value == null ? "" : String(s.value);
    }
  } catch (e) {
    error.value = e.message;
  } finally {
    loading.value = false;
  }
}

function isChecked(id) {
  return values[id] === "true";
}
function toggleBool(id, e) {
  values[id] = e.target.checked ? "true" : "false";
}

async function save() {
  saving.value = true;
  saveError.value = "";
  const fields = {};
  for (const s of cfg.value.settings || []) {
    fields[s.id] = values[s.id];
  }
  try {
    await addIndexer(props.indexerId, fields);
    emit("saved");
  } catch (e) {
    saveError.value = e.message;
  } finally {
    saving.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="modal" @click.self="$emit('close')">
    <div class="modal-box">
      <div class="modal-head">
        <h2>
          {{ cfg ? cfg.name : indexerId }}
          <a v-if="titleLink" :href="titleLink" target="_blank" rel="noopener">{{ hostLabel }}</a>
        </h2>
        <button type="button" class="modal-close" aria-label="关闭" @click="$emit('close')">×</button>
      </div>

      <div v-if="loading" class="modal-desc">加载中…</div>
      <div v-else-if="error" class="modal-desc alert alert-err">{{ error }}</div>

      <template v-else>
        <div class="modal-body">
          <div v-if="cfg.description" class="alert alert-info">{{ cfg.description }}</div>

          <form v-if="cfg.settings && cfg.settings.length" class="config-setup-form">
            <div v-for="s in cfg.settings" :key="s.id" class="setup-item">
              <div class="setup-item-label">{{ s.label || s.id }}</div>
              <div class="setup-item-value">
                <input
                  v-if="s.type === 'inputstring'"
                  class="form-control"
                  type="text"
                  v-model="values[s.id]"
                />
                <input
                  v-else-if="s.type === 'inputpassword'"
                  class="form-control"
                  type="password"
                  v-model="values[s.id]"
                />
                <input
                  v-else-if="s.type === 'inputtags'"
                  class="form-control"
                  type="text"
                  v-model="values[s.id]"
                  placeholder="逗号分隔"
                />
                <label v-else-if="s.type === 'inputbool'" class="checkbox-label">
                  <input
                    type="checkbox"
                    :checked="isChecked(s.id)"
                    @change="toggleBool(s.id, $event)"
                  />
                  {{ values[s.id] === "true" ? "是" : "否" }}
                </label>
                <select
                  v-else-if="s.type === 'inputselect'"
                  class="form-control"
                  v-model="values[s.id]"
                >
                  <option v-for="(v, k) in s.options" :key="k" :value="k">{{ v }}</option>
                </select>
                <div v-else-if="s.type === 'displayinfo'" class="alert alert-info">
                  {{ values[s.id] }}
                </div>
                <input v-else class="form-control" type="text" v-model="values[s.id]" />
              </div>
            </div>
          </form>
          <p v-else class="muted">该索引器没有可编辑的配置项。</p>

          <h2>Capabilities</h2>
          <div class="table-wrap">
            <table class="caps-table">
              <thead>
                <tr><th>Category</th><th>Description</th></tr>
              </thead>
              <tbody>
                <tr v-for="c in cfg.caps" :key="c.id">
                  <td>{{ c.id }}</td>
                  <td>{{ c.desc }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div v-if="saveError" class="alert alert-err" style="margin: 0 18px">{{ saveError }}</div>
        <div class="modal-foot">
          <button type="button" class="navbtn" @click="$emit('close')">Close</button>
          <button type="button" class="navbtn primary" :disabled="saving" @click="save">
            {{ saving ? "Saving…" : "Okay" }}
          </button>
        </div>
      </template>
    </div>
  </div>
</template>
