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

const props = defineProps<{ resource: any; providers: any[] }>()
const emit = defineEmits<{ (e: 'saved'): void; (e: 'cancel'): void }>()

const { t } = useI18n()

const payload = computed<any>(() => props.resource?.payload || {})
const name = ref(props.resource?.name || '')
const enabled = ref(Boolean(props.resource?.enabled))
const busy = ref(false)
const error = ref('')

const provider = computed(() => props.providers.find((p) => p.id === props.resource?.parentId))

/** 三态布尔：'' = 跟随服务商，'true' / 'false' 显式下发。 */
function tri(v: unknown): string {
  if (v === true) return 'true'
  if (v === false) return 'false'
  return ''
}

const caps = ref({
  supportsVision: tri(payload.value.supportsVision),
  supportsAudio: tri(payload.value.supportsAudio),
  canGenerateImage: tri(payload.value.canGenerateImage),
  canGenerateVideo: tri(payload.value.canGenerateVideo),
})

const limits = ref({
  contextBudgetTokens: payload.value.contextBudgetTokens ?? '',
  maxTokens: payload.value.maxTokens ?? '',
  temperature: payload.value.temperature ?? '',
  topP: payload.value.topP ?? '',
})

const thinking = ref({
  enableThinking: tri(payload.value.enableThinking),
  thinkingIntensity: payload.value.thinkingIntensity ?? '',
  thinkingBudget: payload.value.thinkingBudget ?? '',
  reasoningEffort: payload.value.reasoningEffort ?? '',
  thinkingProtocol: payload.value.thinkingProtocol ?? '',
  reasoningInMessages: tri(payload.value.reasoningInMessages),
})

const INTENSITY = ['', 'off', 'low', 'medium', 'high', 'max']
const EFFORT = ['', 'low', 'medium', 'high', 'max']

function intensityLabel(v: string) {
  return v ? t(`tier.thinking_${v}`) : t('models.follow')
}

async function submit() {
  error.value = ''
  busy.value = true
  try {
    // 只把填了的字段写进 payload：留空 = 不下发，客户端继续继承服务商默认。
    const next: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(caps.value)) if (v !== '') next[k] = v === 'true'
    for (const [k, v] of Object.entries(limits.value)) if (v !== '' && v !== null) next[k] = Number(v)
    for (const [k, v] of Object.entries(thinking.value)) {
      if (v === '' || v === null) continue
      next[k] = k === 'enableThinking' || k === 'reasoningInMessages' ? v === 'true' : v
    }

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
          <label v-for="(labelKey, key) in {
            supportsVision: 'models.vision',
            supportsAudio: 'models.audio',
            canGenerateImage: 'models.imageOut',
            canGenerateVideo: 'models.videoOut',
          }" :key="key" class="cap">
            <span>{{ t(labelKey) }}</span>
            <select v-model="caps[key as keyof typeof caps]">
              <option value="">{{ t('models.follow') }}</option>
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
            <input v-model="limits.contextBudgetTokens" type="number" min="0" :placeholder="t('models.follow')" />
          </div>
          <div>
            <label>{{ t('models.maxTokens') }}</label>
            <input v-model="limits.maxTokens" type="number" min="0" :placeholder="t('models.follow')" />
          </div>
          <div>
            <label>{{ t('models.temperature') }}</label>
            <input v-model="limits.temperature" type="number" step="0.1" min="0" max="2" :placeholder="t('models.follow')" />
          </div>
          <div>
            <label>{{ t('models.topP') }}</label>
            <input v-model="limits.topP" type="number" step="0.05" min="0" max="1" :placeholder="t('models.follow')" />
          </div>
        </div>
      </fieldset>

      <fieldset>
        <legend>{{ t('models.thinking') }}</legend>
        <div class="three">
          <div>
            <label>{{ t('models.enableThinking') }}</label>
            <select v-model="thinking.enableThinking">
              <option value="">{{ t('models.follow') }}</option>
              <option value="true">{{ t('models.yes') }}</option>
              <option value="false">{{ t('models.no') }}</option>
            </select>
          </div>
          <div>
            <label>{{ t('tier.thinking') }}</label>
            <select v-model="thinking.thinkingIntensity">
              <option v-for="v in INTENSITY" :key="v" :value="v">{{ intensityLabel(v) }}</option>
            </select>
          </div>
          <div>
            <label>{{ t('models.thinkingBudget') }}</label>
            <input v-model="thinking.thinkingBudget" type="number" min="0" :placeholder="t('models.follow')" />
          </div>
          <div>
            <label>reasoningEffort</label>
            <select v-model="thinking.reasoningEffort">
              <option v-for="v in EFFORT" :key="v" :value="v">{{ v || t('models.follow') }}</option>
            </select>
          </div>
          <div>
            <label>thinkingProtocol</label>
            <input v-model="thinking.thinkingProtocol" :placeholder="t('models.follow')" />
          </div>
          <div>
            <label>{{ t('models.reasoningInMessages') }}</label>
            <select v-model="thinking.reasoningInMessages">
              <option value="">{{ t('models.follow') }}</option>
              <option value="true">{{ t('models.yes') }}</option>
              <option value="false">{{ t('models.no') }}</option>
            </select>
          </div>
        </div>
      </fieldset>

      <p class="small muted" style="margin-top: 10px">{{ t('models.followHint') }}</p>

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
