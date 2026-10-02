<script setup lang="ts">
import DefaultTheme from 'vitepress/theme';
import { useData, withBase } from 'vitepress';

const { Layout } = DefaultTheme;
const { frontmatter } = useData();
</script>

<template>
  <Layout>
    <template #nav-bar-content-after>
      <a v-if="frontmatter.altLocale" class="pointer-lang-switch" :href="withBase(frontmatter.altLocale.link)">
        {{ frontmatter.altLocale.text }}
      </a>
    </template>
    <template #nav-screen-content-after>
      <a v-if="frontmatter.altLocale" class="pointer-lang-switch pointer-lang-switch-screen" :href="withBase(frontmatter.altLocale.link)">
        {{ frontmatter.altLocale.text }}
      </a>
    </template>
    <template #doc-before>
      <!-- Injected by transformPageData for tier-2 sections; the docs themselves
           carry no frontmatter. -->
      <div v-if="frontmatter.maintainerNote" class="pointer-maintainer-note" v-html="frontmatter.maintainerNote" />
    </template>
  </Layout>
</template>
