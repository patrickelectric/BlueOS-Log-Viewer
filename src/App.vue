<template>
  <div id="app">
    <h2>BlueOS System Log Viewer</h2>
    <file-upload @logs-updated="updateLogs" @processing="processing"></file-upload>
    <div v-if= "currentMsg">
      Processing: {{currentMsg}}
    </div>
    <div v-if="allLogs.length">
      <select v-model="selectedService">
        <option v-for="service in availableServices.sort()" :key="service" :value="service">{{ service }}</option>
      </select>
      <log-viewer :logData="filteredLogs"></log-viewer>
    </div>
    <div v-else-if="currentMsg">Loading...</div>
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
    const currentMsg = ref('')

    const updateLogs = (newLogs) => {
      allLogs.value = newLogs;
      selectedService.value = newLogs.length > 0 ? newLogs[0].serviceName : '';
      currentMsg.value = '';
    };

    const processing = (msg) => {
      currentMsg.value = msg;
    }

    const filteredLogs = computed(() => {
      return allLogs.value.filter(log => log.serviceName === selectedService.value);
    });

    const availableServices = computed(() => {
      return [...new Set(allLogs.value.map(log => log.serviceName))];
    });

    return { allLogs, selectedService, currentMsg, filteredLogs, availableServices, updateLogs, processing };
  }
};
</script>