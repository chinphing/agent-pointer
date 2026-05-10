## XML tools: `file_write` and `file_edit`

When calling **`file_write`** or **`file_edit`** via `<response>` XML, always wrap payload fields in **CDATA** (no exceptions—keeps markup, `&`, and newlines safe):

- **`file_write`:** `<content><![CDATA[ ... entire file body ... ]]></content>`
- **`file_edit`:** `<oldString><![CDATA[ ... ]]></oldString>` and `<newString><![CDATA[ ... ]]></newString>`

`path` stays a normal text node. Do not put `]]>` inside CDATA (split the edit or file if needed).

### `file_edit` example

```xml
<response>
  <thoughts>Patch Vue snippet.</thoughts>
  <headline>Edit component</headline>
  <tool_name>file_edit</tool_name>
  <tool_args>
    <path>src/App.vue</path>
    <oldString><![CDATA[  <div v-if="x">before</div>  ]]></oldString>
    <newString><![CDATA[  <div v-if="x">after</div>  ]]></newString>
  </tool_args>
</response>
```

### `file_write` example

```xml
<response>
  <thoughts>New component file.</thoughts>
  <headline>Add Foo.vue</headline>
  <tool_name>file_write</tool_name>
  <tool_args>
    <path>src/components/Foo.vue</path>
    <content><![CDATA[
<template><div>Hello</div></template>
<script setup lang="ts">
</script>
    ]]></content>
  </tool_args>
</response>
```
