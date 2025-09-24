// Worker thread script for libagent FFI calls
// This runs in a separate thread to provide truly async FFI operations

const { parentPort, workerData } = require('worker_threads');

// Import the FFI setup from the main module
const { lib, ref, ResponseCallback, ErrorCallback, size_t, uint16, uint8Ptr, charPtr, voidPtr } = require('./index');

// Extract request parameters from workerData
const { method, path, headers, body } = workerData;

let completed = false;

// Callback implementations for this worker
const responseCallback = ResponseCallback((status, headersData, headersLen, bodyData, bodyLen, userData) => {
  if (completed) return;
  completed = true;

  try {
    // Extract headers
    let responseHeaders = '';
    if (headersData && !headersData.isNull() && headersLen > 0) {
      const headersBuf = ref.reinterpret(headersData, headersLen);
      responseHeaders = headersBuf.toString('utf8');
    }

    // Extract body
    let responseBody = Buffer.alloc(0);
    if (bodyData && !bodyData.isNull() && bodyLen > 0) {
      responseBody = ref.reinterpret(bodyData, bodyLen);
    }

    // Send success response back to main thread
    parentPort.postMessage({
      type: 'success',
      response: {
        status,
        headers: responseHeaders,
        body: responseBody
      }
    });
  } catch (err) {
    parentPort.postMessage({
      type: 'error',
      error: `Response processing error: ${err.message}`
    });
  }
});

const errorCallback = ErrorCallback((errorMessage, userData) => {
  if (completed) return;
  completed = true;

  let errorMsg = 'Unknown error';
  if (errorMessage && !errorMessage.isNull()) {
    errorMsg = errorMessage.readCString(0);
  }

  parentPort.postMessage({
    type: 'error',
    error: errorMsg
  });
});

// Execute the FFI call
try {
  // Prepare body data
  let bodyPtr = ref.NULL;
  let bodyLen = 0;

  if (body && body.length > 0) {
    bodyPtr = ref.alloc(ref.types.uint8, body.length);
    ref.writePointer(bodyPtr, 0, body);
    bodyLen = body.length;
  }

  // Make the FFI call
  const rc = lib.ProxyTraceAgent(
    method,
    path,
    headers,
    bodyPtr,
    bodyLen,
    responseCallback,
    errorCallback,
    ref.NULL
  );

  // Handle immediate errors (though callbacks should handle most cases)
  if (rc !== 0 && !completed) {
    completed = true;
    parentPort.postMessage({
      type: 'error',
      error: `ProxyTraceAgent returned error code: ${rc}`
    });
  }

} catch (err) {
  if (!completed) {
    completed = true;
    parentPort.postMessage({
      type: 'error',
      error: `Worker execution error: ${err.message}`
    });
  }
}

// Clean up callbacks when done
process.on('exit', () => {
  try {
    responseCallback.dispose();
    errorCallback.dispose();
  } catch (err) {
    // Ignore cleanup errors
  }
});
