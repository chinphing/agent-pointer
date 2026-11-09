<script setup lang="ts">
/**
 * 模型编辑。
 *
 * 模型不是「有名字的资源」那么简单——它带一组能力与生成参数，客户端
 * （`PlatformProviderModel`）认这些字段：能力开关、上下文预算、思考强度……
 * 通用资源表单里只有一个 payload JSON 文本框，等于让人手写这些参数。
 *
 * 这里把字段摊开成表单；留空 = 不下发 = 继承服务商默认。
 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { CAP_KEYS, LIMIT_KEYS, THINKING_KEYS, buildModelPayload, payloadToState } from '../lib/modelPayload'
import { FIELD_DEFAULTS, contextBudgetLabel } from '../lib/modelDefaults'

const props = defineProps<{ resource: any; providers: any[] }>()
const emit = defineEmits<{ (e: 'saved'): void; (e: 'cancel'): void }>()

const { t } = useI18n()

const payload = computed<any>(() => props.resource?.payload || {})
const name = ref(props.resource?.name || '')
const enabled = ref(Boolean(props.resource?.enabled))
const busy = ref(false)
const error = ref('')

const provider = computed(() => props.providers.find((p) => p.id === props.resource?.parentId))

const initial = payloadToState(payload.value)
const caps = ref<Record<string, string>>(initial.caps)
const limits = ref<Record<string, string>>(initial.limits)
const thinking = ref<Record<string, string>>(initial.thinking)

const CAP_LABELS: Record<string, string> = {
  supportsVision: 'vision',
  supportsAudio: 'audio',
  canGenerateImage: 'imageOut',
  canGenerateVideo: 'videoOut',
}
const INTENSITY = ['', 'off', 'low', 'medium', 'high', 'max']
/** 客户端 normalize_reasoning_effort 只认 high / max，其余值会被丢掉。 */

function intensityLabel(v: string) {
  return v ? t(`tier.thinking_${v}`) : t('models.follow')
}

/** 强度留空时客户端按思考预算反推（2048 → 中），不是固定某一档。 */

function ph(v: string | number) {
  return t('models.defaultIs', { v })
}

async function submit() {
  error.value = ''
  busy.value = true
  try {
    const next = buildModelPayload({
      caps: caps.value,
      limits: limits.value,
      thinking: thinking.value,
    })

    await api.put(`/api/admin/resources/${props.resource.id}`, {
      name: name.value.trim() || props.resource.slug,
      enabled: enabled.value,
      payload: next,
    })
    emit('saved')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="modal-mask" @click.self="emit('cancel')">
    <div class="modal" style="width: 560px">
      <h2>{{ t('models.editTitle') }}</h2>

      <div class="ident">
        <span class="mono">{{ resource.slug }}</span>
        <span class="small muted">{{ provider?.name || t('resources.orphanProvider') }}</span>
      </div>

      <div class="two">
        <div>
          <label>{{ t('models.displayName') }}</label>
          <input v-model="name" :placeholder="resource.slug" />
        </div>
        <div>
          <label>{{ t('resources.status') }}</label>
          <select v-model="enabled">
            <option :value="true">{{ t('common.enabled') }}</option>
            <option :value="false">{{ t('common.disabled') }}</option>
          </select>
        </div>
      </div>

      <fieldset>
        <legend>{{ t('models.capabilities') }}</legend>
        <div class="cap-grid">
          <label v-for="key in CAP_KEYS" :key="key" class="cap">
            <span>{{ t(`models.${CAP_LABELS[key]}`) }}</span>
            <select v-model="caps[key]">
              <option value="">{{ t('models.unset') }}</option>
              <option value="true">{{ t('models.yes') }}</option>
              <option value="false">{{ t('models.no') }}</option>
            </select>
          </label>
        </div>
      </fieldset>

      <fieldset>
        <legend>{{ t('models.limits') }}</legend>
        <div class="two">
          <div>
            <label>{{ t('models.contextBudget') }}</label>
            <input v-model="limits.contextBudgetTokens" type="number" min="0" :placeholder="contextBudgetLabel()" />
          </div>
          <div>
            <label>{{ t('models.maxTokens') }}</label>
            <input v-model="limits.maxTokens" type="number" min="0" :placeholder="ph(FIELD_DEFAULTS.maxTokens)" />
          </div>
          <div>
            <label>{{ t('models.temperature') }}</label>
            <input v-model="limits.temperature" type="number" step="0.05" min="0" max="2" :placeholder="ph(FIELD_DEFAULTS.temperature)" />
          </div>
          <div>
            <label>{{ t('models.topP') }}</label>
            <input v-model="limits.topP" type="number" step="0.05" min="0" max="1" :placeholder="ph(FIELD_DEFAULTS.topP)" />
          </div>
        </div>
      </fieldset>

      <fieldset>
        <legend>{{ t('models.thinking') }}</legend>
        <div class="three">
          <div>
            <label>{{ t('tier.thinking') }}</label>
            <select v-model="thinking.thinkingIntensity">
              <option v-for="v in INTENSITY" :key="v" :value="v">{{ intensityLabel(v) }}</option>
            </select>
          </div>
          <div>
            <label>{{ t('models.reasoningInMessages') }}</label>
            <select v-model="thinking.reasoningInMessages">
              <option value="">{{ t('models.noDefault') }}</option>
              <option value="true">{{ t('models.yes') }}</option>
              <option value="false">{{ t('models.no') }}</option>
            </select>
          </div>
        </div>
      </fieldset>

      <p class="small muted" style="margin-top: 10px">{{ t('models.defaultsNote') }}</p>

      <div v-if="error" class="err" style="margin-top: 10px">{{ error }}</div>

      <div class="modal-actions">
        <button @click="emit('cancel')">{{ t('common.cancel') }}</button>
        <button class="primary" :disabled="busy" @click="submit">{{ t('common.save') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.ident {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-bottom: 12px;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--line);
}
.two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.three { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; }
fieldset {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 8px 12px 10px;
  margin: 12px 0 0;
}
legend { font-size: 12px; color: var(--muted); padding: 0 6px; }
.cap-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
.cap { display: flex; flex-direction: column; gap: 4px; margin: 0; }
.cap select { width: 100%; }
</style>
