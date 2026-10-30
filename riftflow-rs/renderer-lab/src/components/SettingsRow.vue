<template>
  <div
    class="settings-row"
    :class="{
      'settings-row--center': align === 'center',
      'settings-row--start': align === 'start',
      'settings-row--disabled': disabled,
      'settings-row--control-full-line': controlFullLine,
      'settings-row--no-x-padding': noXPadding,
    }"
    :style="{
      '--settings-row-label-width': labelWidth ? `${labelWidth}px` : '220px',
      '--settings-row-gap': gap ? `${gap}px` : '48px',
      '--settings-row-label-min-width': labelMinWidth ? `${labelMinWidth}px` : '0px',
    }"
  >
    <div class="settings-row-label">
      <div class="settings-row-label-main">
        <slot name="label">{{ label }}</slot>
      </div>
      <div v-if="$slots.labelDescription || labelDescription" class="settings-row-label-description">
        <slot name="labelDescription">{{ labelDescription }}</slot>
      </div>
    </div>
    <div class="settings-row-control">
      <slot></slot>
    </div>
  </div>
</template>

<script setup>
defineProps({
  label: String,
  labelDescription: String,
  labelWidth: Number,
  labelMinWidth: Number,
  gap: Number,
  noXPadding: Boolean,
  controlFullLine: Boolean,
  align: { type: String, default: 'center' },
  disabled: Boolean,
})
</script>

<style>
/* Values transcribed from the original `SettingsRow.vue`, whose classes are Tailwind
 * utilities: min-h-13 → 3.25rem, py-3 → .75rem, text-sm → 14px/20px, mt-0.5 → 2px. */
.settings-row {
  box-sizing: border-box;
  display: flex;
  width: 100%;
  max-width: 100%;
  min-height: 3.25rem;
  padding: 0.75rem var(--settings-row-x-padding, 12px);
  column-gap: var(--settings-row-gap);
  border-bottom: 1px solid rgb(0 0 0 / 5%);
}

.settings-row--no-x-padding {
  padding-left: 0;
  padding-right: 0;
}

.settings-row:last-child {
  border-bottom: 0;
}

.settings-row--center {
  align-items: center;
}

.settings-row--start {
  align-items: flex-start;
}

.settings-row--control-full-line {
  row-gap: 8px;
  flex-wrap: wrap;
}

.settings-row-label {
  flex: 1 1 var(--settings-row-label-width);
  min-width: min(var(--settings-row-label-min-width), 100%);
}

.settings-row--control-full-line .settings-row-label {
  flex-basis: 100%;
}

.settings-row-label-main {
  font-size: 14px;
  line-height: 20px;
  font-weight: 400;
  color: rgb(0 0 0 / 80%);
}

.settings-row-label-description {
  margin-top: 2px;
  font-size: 13px;
  color: rgb(0 0 0 / 60%);
}

.settings-row--disabled .settings-row-label-main,
.settings-row--disabled .settings-row-label-description {
  color: rgb(0 0 0 / 50%);
}

.settings-row-control {
  flex: 0 1 auto;
  display: flex;
  min-width: 0;
  justify-content: flex-end;
  max-width: calc(100% - var(--settings-row-gap));
}

.settings-row--control-full-line .settings-row-control {
  justify-content: flex-start;
  max-width: 100%;
  flex-basis: 100%;
}

.dark .settings-row {
  border-bottom-color: rgb(255 255 255 / 10%);
}

.dark .settings-row-label-main {
  color: rgb(255 255 255 / 90%);
}

.dark .settings-row-label-description {
  color: rgb(255 255 255 / 60%);
}

.dark .settings-row--disabled .settings-row-label-main,
.dark .settings-row--disabled .settings-row-label-description {
  color: rgb(255 255 255 / 50%);
}
</style>
