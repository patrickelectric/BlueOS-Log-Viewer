<template>
  <div id="app">
    <h2>BlueOS System Log Viewer</h2>
    <file-upload @logs-updated="updateLogs"></file-upload>
    <div v-if="allLogs.length">
      <select v-model="selectedService">
        <option v-for="service in availableServices" :key="service" :value="service">{{ service }}</option>
      </select>
      <log-viewer :logData="filteredLogs"></log-viewer>
    </div>
    <div v-else>No logs available.</div>
  </div>
</template>

<script>
import { ref, computed } from 'vue';
import FileUpload from './components/FileUpload.vue';
import LogViewer from './components/LogViewer.vue';

export default {
  components: {
    FileUpload,
    LogViewer
  },
  setup() {
    const allLogs = ref([]);
    const selectedService = ref('');

    const updateLogs = (newLogs) => {
      allLogs.value = newLogs;
      selectedService.value = newLogs.length > 0 ? newLogs[0].serviceName : '';
    };

    const filteredLogs = computed(() => {
      return allLogs.value.filter(log => log.serviceName === selectedService.value);
    });

    const availableServices = computed(() => {
      return [...new Set(allLogs.value.map(log => log.serviceName))];
    });

    return { allLogs, selectedService, filteredLogs, availableServices, updateLogs };
  }
};
</script>