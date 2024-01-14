<template>
  <div class="log-viewer">
    <div v-if="logData && logData.length" class="log-content">
      <div v-for="(log, index) in logData" :key="index" class="log-entry">
        <h3>{{ log.serviceName }} - {{ formatDate(log.dateTime) }}</h3>
        <pre>{{ log.content }}</pre>
      </div>
    </div>
    <div v-else class="no-log">
      No logs to display.
    </div>
  </div>
</template>

<script>
import { defineProps } from 'vue';

export default {
  props: {
    logData: Array
  },
  setup() {
    const props = defineProps({
      logData: {
        type: Array,
        default: () => []
      }
    });

    const formatDate = (date) => {
      if (date) {
        return date.toLocaleString();
      }
      return '';
    };

    return { ...props, formatDate };
  }
};
</script>

<style scoped>
.log-viewer {
  margin: 20px;
  padding: 10px;
  border: 1px solid #ccc;
  border-radius: 5px;
}

.log-content pre {
  background-color: #f4f4f4;
  padding: 10px;
  border-radius: 5px;
  white-space: pre-wrap;
  word-break: break-word;
}

.log-entry {
  margin-bottom: 20px;
}

.no-log {
  color: #888;
}
</style>