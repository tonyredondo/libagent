// npm install ffi-napi ref-napi
// Truly async JavaScript API using worker threads
// This provides non-blocking FFI calls by running them on background threads

const { Worker } = require('worker_threads');
const path = require('path');

// High-level async API using worker threads
class AsyncLibAgentClient {
  // Make a request and return a Promise (truly async with worker threads)
  static request(method, path, headers = '', body = null) {
    return new Promise((resolve, reject) => {
      // Create a worker thread for this request
      const workerPath = path.join(__dirname, 'libagent-worker.js');
      const worker = new Worker(workerPath);

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

// Example usage with async/await
async function main() {
  try {
    console.log('=== Async Worker Thread Example ===');
    console.log('This uses worker threads for truly non-blocking FFI calls!');

    // Example 1: Simple GET request
    console.log('\n--- GET Request ---');
    const response = await AsyncLibAgentClient.get('/info', 'Accept: application/json\n');
    console.log('Status:', response.status);
    console.log('Headers:', response.headers);
    console.log('Body:', response.body.toString());

    // Example 2: POST request with body
    console.log('\n--- POST Request ---');
    const postBody = Buffer.from(JSON.stringify({ key: 'value' }));
    const postResponse = await AsyncLibAgentClient.post(
      '/api/test',
      'Content-Type: application/json\nAccept: application/json\n',
      postBody
    );
    console.log('POST Status:', postResponse.status);

    // Example 3: Multiple concurrent requests
    console.log('\n--- Concurrent Requests ---');
    const requests = [
      AsyncLibAgentClient.get('/api/users', 'Accept: application/json\n'),
      AsyncLibAgentClient.get('/api/posts', 'Accept: application/json\n'),
      AsyncLibAgentClient.post('/api/notify', 'Content-Type: application/json\n',
                              Buffer.from(JSON.stringify({ message: 'Hello' })))
    ];

    const results = await Promise.all(requests);
    console.log('All requests completed:');
    results.forEach((result, i) => {
      console.log(`  Request ${i + 1}: Status ${result.status}`);
    });

  } catch (error) {
    console.error('Error:', error.message);
  }
}

// Run if called directly
if (require.main === module) {
  main();
}

module.exports = AsyncLibAgentClient;
