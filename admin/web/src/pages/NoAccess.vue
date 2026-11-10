<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
</script>

<template>
  <div class="empty" style="margin-top: 48px">
    <div style="font-size: 15px; color: var(--fg)">{{ t('noAccess.title') }}</div>
    <p class="muted small" style="margin-top: 8px">{{ t('noAccess.hint') }}</p>
  </div>
</template>
