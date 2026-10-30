<template>
  <section class="settings-section" :class="{ 'settings-section--no-bg': noBg }">
    <header class="settings-section-header">
      <slot name="header">
        <span class="settings-section-title">{{ title }}</span>
      </slot>
    </header>
    <div class="settings-section-body">
      <slot />
    </div>
    <footer v-if="$slots.footer || footer" class="settings-section-footer">
      <slot name="footer">{{ footer }}</slot>
    </footer>
  </section>
</template>

<script setup>
defineProps({
  noBg: Boolean,
  title: String,
  footer: String,
})
</script>

<style>
.settings-section {
  --settings-row-x-padding: 12px;

  box-sizing: border-box;
  width: 100%;
  max-width: 100%;
}

.settings-section-header {
  margin-bottom: 8px;
  padding: 0 var(--settings-row-x-padding);
}

.settings-section-title {
  font-size: 14px;
  line-height: 20px;
  font-weight: 700;
  color: rgb(0 0 0 / 80%);
}

.settings-section-body {
  width: 100%;
  max-width: 100%;
  overflow: hidden;
  border-radius: 8px;
}

.settings-section:not(.settings-section--no-bg) .settings-section-body {
  background-color: rgb(0 0 0 / 5%);
}

.settings-section-footer {
  margin-top: 4px;
  padding: 0 var(--settings-row-x-padding);
  font-size: 12px;
  line-height: 1.375;
  color: rgb(0 0 0 / 55%);
}

.dark .settings-section-title {
  color: rgb(255 255 255 / 90%);
}

.dark .settings-section:not(.settings-section--no-bg) .settings-section-body {
  background-color: rgb(255 255 255 / 8%);
}

.dark .settings-section-footer {
  color: rgb(255 255 255 / 55%);
}
</style>
