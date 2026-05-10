## XML tools: `file:write` and `file:edit`

When calling **`file:write`** or **`file:edit`** via `<response>` XML, always wrap payload fields in **CDATA** (no exceptions—keeps markup, `&`, and newlines safe). You may also use `<tool_name>file</tool_name>` with a `<method>` argument (`write` / `edit`).

- **`file:write`:** `<content><![CDATA[ ... entire file body ... ]]></content>`
- **`file:edit`:** `<oldString><![CDATA[ ... ]]></oldString>` and `<newString><![CDATA[ ... ]]></newString>`

`path` stays a normal text node. Do not put `]]>` inside CDATA (split the edit or file if needed).

### `file:edit` example

```xml
<response>
  <thoughts>Patch Vue snippet.</thoughts>
  <headline>Edit component</headline>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>src/App.vue</path>
    <oldString><![CDATA[  <div v-if="x">before</div>  ]]></oldString>
    <newString><![CDATA[  <div v-if="x">after</div>  ]]></newString>
  </tool_args>
</response>
```

### `file:write` example

```xml
<response>
  <thoughts>New component file.</thoughts>
  <headline>Add Foo.vue</headline>
  <tool_name>file:write</tool_name>
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
