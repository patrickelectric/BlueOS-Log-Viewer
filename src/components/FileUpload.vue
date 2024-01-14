<template>
  <div class="file-upload">
    <input type="file" @change="handleFileUpload" accept=".zip" />
  </div>
</template>

<script>
import { ref } from 'vue';
import JSZip from 'jszip';
import pako from 'pako';

export default {
  emits: ['logs-updated'],
  setup(props, { emit }) {
    const fileContent = ref(null);

    const handleFileUpload = async (event) => {
      const file = event.target.files[0];
      if (!file) {
        return;
      }

      const zip = await JSZip.loadAsync(file);
      const allLogs = [];

      for (const fileName of Object.keys(zip.files)) {
        console.log(`${fileName}`)
        const serviceName = extractServiceName(fileName);
        const dateTime = extractDateTime(fileName);
        console.log(`${serviceName} -> ${dateTime}`)
        if (dateTime == null) {
          continue
        }

        if (fileName.endsWith('.gz')) {
          const gzippedData = await zip.files[fileName].async('uint8array');
          let content = `File: ${fileName}\n\n`
          content += pako.inflate(gzippedData, { to: 'string' });
          allLogs.push({ serviceName, dateTime, content });
        } else if (fileName.endsWith('.log')) {
          let content = `File: ${fileName}\n\n`
          content += await zip.files[fileName].async("string");
          allLogs.push({ serviceName, dateTime, content });
        }
      }

      allLogs.sort((a, b) =>  b.dateTime - a.dateTime);
      emit('logs-updated', allLogs);
    };

    return { fileContent, handleFileUpload };
  }
};

function extractServiceName(fileName) {
  const parts = fileName.split('/');
  return parts[0];
}

function extractDateTime(fileName) {
  let regex, match, dateStr, timeStr;

  if (fileName.endsWith('.log')) {
    // Regex for 'MM-DD-YYYY_HH-MM-SS.log' format
    regex = /(\d{2}-\d{2}-\d{4})_(\d{2}:\d{2}:\d{2})\.log/;
    match = fileName.match(regex);
    console.log(match)

    if (match && match.length > 2) {
      // Transform 'MM-DD-YYYY' to 'YYYY-MM-DD'
      dateStr = `${match[1].substring(6, 10)}-${match[1].substring(0, 2)}-${match[1].substring(3, 5)}`;
      timeStr = match[2].replace(/-/g, ':'); // Replace '-' with ':' in 'HH-MM-SS'
    }
  } else {
    // Regex for 'service-name-YYYYMMDD-HHMMSS.zip/gz' format
    regex = /(\d{8})-(\d{6})\.(zip|gz)/;
    match = fileName.match(regex);

    if (match && match.length > 2) {
      // Transform 'YYYYMMDD' to 'YYYY-MM-DD'
      dateStr = `${match[1].substring(0, 4)}-${match[1].substring(4, 6)}-${match[1].substring(6, 8)}`;
      // Transform 'HHMMSS' to 'HH:MM:SS'
      timeStr = `${match[2].substring(0, 2)}:${match[2].substring(2, 4)}:${match[2].substring(4, 6)}`;
    }
  }

  if (dateStr && timeStr) {
    // Combine date and time to create an ISO 8601 formatted string
    const isoFormattedString = `${dateStr}T${timeStr}`;
    console.log(isoFormattedString)
    return new Date(isoFormattedString);
  }

  return null;
}
</script>
