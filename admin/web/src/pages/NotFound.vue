<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

const { t } = useI18n()
const router = useRouter()
</script>

<template>
  <div class="empty" style="margin-top: 48px">
    <h1 style="font-size: 15px; color: var(--fg)">{{ t('notFound.title') }}</h1>
    <p class="muted small" style="margin-top: 8px">{{ t('notFound.hint') }}</p>
    <button class="btn" style="margin-top: 16px" @click="router.push('/')">
      {{ t('notFound.back') }}
    </button>
  </div>
</template>
