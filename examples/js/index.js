// npm install ffi-napi ref-napi
const ffi = require('ffi-napi');
const ref = require('ref-napi');

const size_t = ref.types.size_t;
const uint16 = ref.types.uint16;
const uint8Ptr = ref.refType(ref.types.uint8);
const charPtr = ref.refType(ref.types.char);
const voidPtr = ref.refType(ref.types.void);

// Define callback types
const ResponseCallback = ffi.Callback('void', [uint16, uint8Ptr, size_t, uint8Ptr, size_t, voidPtr]);
const ErrorCallback = ffi.Callback('void', [charPtr, voidPtr]);

// Define the MetricsData struct layout (24 * 8 bytes = 192 bytes total)
const MetricsDataFields = [
  ['agent_spawns', ref.types.uint64],
  ['trace_agent_spawns', ref.types.uint64],
  ['agent_failures', ref.types.uint64],
  ['trace_agent_failures', ref.types.uint64],
  ['uptime_seconds', ref.types.double],
  ['proxy_get_requests', ref.types.uint64],
  ['proxy_post_requests', ref.types.uint64],
  ['proxy_put_requests', ref.types.uint64],
  ['proxy_delete_requests', ref.types.uint64],
  ['proxy_patch_requests', ref.types.uint64],
  ['proxy_head_requests', ref.types.uint64],
  ['proxy_options_requests', ref.types.uint64],
  ['proxy_other_requests', ref.types.uint64],
  ['proxy_2xx_responses', ref.types.uint64],
  ['proxy_3xx_responses', ref.types.uint64],
  ['proxy_4xx_responses', ref.types.uint64],
  ['proxy_5xx_responses', ref.types.uint64],
  ['response_time_ema_all', ref.types.double],
  ['response_time_ema_2xx', ref.types.double],
  ['response_time_ema_4xx', ref.types.double],
  ['response_time_ema_5xx', ref.types.double],
  ['response_time_sample_count', ref.types.uint64],
];

const MetricsData = ref.refType(ref.types.void); // Buffer to hold struct data

const lib = ffi.Library('libagent', {
  'Initialize': ['void', []],
  'Stop': ['void', []],
  'GetMetrics': [MetricsData, []], // Returns a buffer pointer
  'ProxyTraceAgent': ['int32', ['string', 'string', 'string', voidPtr, size_t, voidPtr, voidPtr, voidPtr]],
});

// Export for use in worker threads
module.exports = { lib, ref, ResponseCallback, ErrorCallback, size_t, uint16, uint8Ptr, charPtr, voidPtr };

// High-level Promise-based API with true async behavior
class LibAgentClient {
  // Make a request and return a Promise (now truly async!)
  static request(method, path, headers = '', body = null) {
    return new Promise((resolve, reject) => {
      // Use worker threads for true async behavior
      const { Worker } = require('worker_threads');

      const worker = new Worker(`
        const { parentPort } = require('worker_threads');
        const ffi = require('ffi-napi');
        const ref = require('ref-napi');

        const size_t = ref.types.size_t;
        const uint16 = ref.types.uint16;
        const uint8Ptr = ref.refType(ref.types.uint8);
        const charPtr = ref.refType(ref.types.char);
        const voidPtr = ref.refType(ref.types.void);

        const ResponseCallback = ffi.Callback('void', [uint16, uint8Ptr, size_t, uint8Ptr, size_t, voidPtr]);
        const ErrorCallback = ffi.Callback('void', [charPtr, voidPtr]);

        const lib = ffi.Library('libagent', {
          'ProxyTraceAgent': ['int32', ['string', 'string', 'string', voidPtr, size_t, voidPtr, voidPtr, voidPtr]],
        });

        // Worker receives the request parameters
        parentPort.on('message', ({ method, path, headers, body }) => {
          let response = null;
          let error = null;
          let completed = false;

          const responseCallback = ResponseCallback((status, headersData, headersLen, bodyData, bodyLen, userData) => {
            try {
              let headers = '';
              if (headersData && !headersData.isNull() && headersLen > 0) {
                const headersBuf = ref.reinterpret(headersData, headersLen);
                headers = headersBuf.toString('utf8');
              }

              let body = Buffer.alloc(0);
              if (bodyData && !bodyData.isNull() && bodyLen > 0) {
                body = ref.reinterpret(bodyData, bodyLen);
              }

              response = { status, headers, body };
              if (!completed) {
                completed = true;
                parentPort.postMessage({ type: 'success', response });
              }
            } catch (err) {
              if (!completed) {
                completed = true;
                parentPort.postMessage({ type: 'error', error: err.message });
              }
            }
          });

          const errorCallback = ErrorCallback((errorMessage, userData) => {
            let errorMsg = 'Unknown error';
            if (errorMessage && !errorMessage.isNull()) {
              errorMsg = errorMessage.readCString(0);
            }

            if (!completed) {
              completed = true;
              parentPort.postMessage({ type: 'error', error: errorMsg });
            }
          });

          try {
            let bodyPtr = ref.NULL;
            let bodyLen = 0;

            if (body && body.length > 0) {
              bodyPtr = ref.alloc(ref.types.uint8, body.length);
              ref.writePointer(bodyPtr, 0, body);
              bodyLen = body.length;
            }

            const rc = lib.ProxyTraceAgent(
              method, path, headers, bodyPtr, bodyLen,
              responseCallback, errorCallback, ref.NULL
            );

            if (rc !== 0 && !completed) {
              completed = true;
              parentPort.postMessage({ type: 'error', error: \`ProxyTraceAgent returned error code: \${rc}\` });
            }

          } catch (err) {
            if (!completed) {
              completed = true;
              parentPort.postMessage({ type: 'error', error: err.message });
            }
          }
        });
      `, { eval: true });

      // Handle worker response
      worker.on('message', (message) => {
        worker.terminate();
        if (message.type === 'success') {
          resolve(message.response);
        } else {
          reject(new Error(message.error));
        }
      });

      worker.on('error', (err) => {
        worker.terminate();
        reject(err);
      });

      worker.on('exit', (code) => {
        if (code !== 0) {
          reject(new Error(`Worker stopped with exit code ${code}`));
        }
      });

      // Send the request to the worker
      worker.postMessage({ method, path, headers, body });
    });
  }

  // Convenience methods
  static get(path, headers = '') {
    return this.request('GET', path, headers);
  }

  static post(path, headers = '', body = null) {
    return this.request('POST', path, headers, body);
  }

  static put(path, headers = '', body = null) {
    return this.request('PUT', path, headers, body);
  }

  static delete(path, headers = '') {
    return this.request('DELETE', path, headers);
  }
}

// Helper function to read metrics from buffer
function readMetrics(buffer) {
  const view = new DataView(buffer.buffer);
  let offset = 0;

  return {
    agent_spawns: Number(view.getBigUint64(offset, true)), offset += 8,
    trace_agent_spawns: Number(view.getBigUint64(offset, true)), offset += 8,
    agent_failures: Number(view.getBigUint64(offset, true)), offset += 8,
    trace_agent_failures: Number(view.getBigUint64(offset, true)), offset += 8,
    uptime_seconds: view.getFloat64(offset, true), offset += 8,
    proxy_get_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_post_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_put_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_delete_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_patch_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_head_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_options_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_other_requests: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_2xx_responses: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_3xx_responses: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_4xx_responses: Number(view.getBigUint64(offset, true)), offset += 8,
    proxy_5xx_responses: Number(view.getBigUint64(offset, true)), offset += 8,
    response_time_ema_all: view.getFloat64(offset, true), offset += 8,
    response_time_ema_2xx: view.getFloat64(offset, true), offset += 8,
    response_time_ema_4xx: view.getFloat64(offset, true), offset += 8,
    response_time_ema_5xx: view.getFloat64(offset, true), offset += 8,
    response_time_sample_count: Number(view.getBigUint64(offset, true)),
  };
}

// Example usage with async/await
async function main() {
  try {
    console.log('Initializing libagent...');
    lib.Initialize();

    console.log('\n=== Initial Metrics ===');
    const initialMetricsBuffer = lib.GetMetrics();
    const initialMetrics = readMetrics(initialMetricsBuffer);
    console.log('Agent spawns:', initialMetrics.agent_spawns);
    console.log('Trace agent spawns:', initialMetrics.trace_agent_spawns);
    console.log('Uptime:', initialMetrics.uptime_seconds.toFixed(2), 'seconds');

    console.log('\n=== Example 1: Simple GET ===');
    const response = await LibAgentClient.get('/info', 'Accept: application/json\n');
    console.log('Status:', response.status);
    console.log('Headers:', response.headers);
    console.log('Body:', response.body.toString());

    console.log('\n=== Example 2: POST with body ===');
    const postBody = Buffer.from(JSON.stringify({ key: 'value' }));
    const postResponse = await LibAgentClient.post(
      '/api/test',
      'Content-Type: application/json\nAccept: application/json\n',
      postBody
    );
    console.log('POST Status:', postResponse.status);

    console.log('\n=== Example 3: Using Promises directly ===');
    await LibAgentClient.request('PUT', '/api/config', 'Content-Type: application/json\n',
                          Buffer.from(JSON.stringify({ setting: true })))
      .then(response => {
        console.log('PUT Status:', response.status);
      })
      .catch(error => {
        console.error('PUT Error:', error.message);
      });

    console.log('\n=== Final Metrics ===');
    const finalMetricsBuffer = lib.GetMetrics();
    const finalMetrics = readMetrics(finalMetricsBuffer);
    console.log('Agent spawns:', finalMetrics.agent_spawns);
    console.log('Trace agent spawns:', finalMetrics.trace_agent_spawns);
    console.log('GET requests:', finalMetrics.proxy_get_requests);
    console.log('POST requests:', finalMetrics.proxy_post_requests);
    console.log('PUT requests:', finalMetrics.proxy_put_requests);
    console.log('2xx responses:', finalMetrics.proxy_2xx_responses);
    console.log('4xx responses:', finalMetrics.proxy_4xx_responses);
    console.log('5xx responses:', finalMetrics.proxy_5xx_responses);
    console.log('Avg response time:', finalMetrics.response_time_ema_all.toFixed(2), 'ms');
    console.log('Uptime:', finalMetrics.uptime_seconds.toFixed(2), 'seconds');

    console.log('\nStopping libagent...');
    lib.Stop();

  } catch (error) {
    console.error('Error:', error.message);
    lib.Stop();
  }
}

// Alternative: Low-level callback API (still available)
function lowLevelExample() {
  const responseCallback = ResponseCallback((status, headersData, headersLen, bodyData, bodyLen, userData) => {
    console.log('Low-level Status:', status);
    // ... callback implementation ...
  });

  const errorCallback = ErrorCallback((errorMessage, userData) => {
    const error = errorMessage && !errorMessage.isNull()
      ? errorMessage.readCString(0)
      : 'Unknown error';
    console.error('Low-level error:', error);
  });

  const rc = lib.ProxyTraceAgent(
    'GET', '/info', 'Accept: application/json\n',
    ref.NULL, 0,
    responseCallback, errorCallback, ref.NULL
  );

  console.log('Low-level return code:', rc);

  // Clean up
  setTimeout(() => {
    responseCallback.dispose();
    errorCallback.dispose();
  }, 100);
}

// Run the main example
main();
