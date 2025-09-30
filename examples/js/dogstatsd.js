/**
 * Example: Send DogStatsD metrics from Node.js using libagent
 * 
 * Install dependencies:
 *   npm install ffi-napi ref-napi
 * 
 * Run:
 *   DYLD_LIBRARY_PATH=../../target/debug node dogstatsd.js
 */

const ffi = require('ffi-napi');
const ref = require('ref-napi');
const path = require('path');

// Determine library extension
const libExt = process.platform === 'darwin' ? 'dylib' : 
               process.platform === 'win32' ? 'dll' : 'so';

const libPath = path.join(__dirname, '../../target/debug', `liblibagent.${libExt}`);

// Define MetricsData structure
const MetricsData = ref.types.void; // We'll just use it as a buffer for this example

// Load libagent
const libagent = ffi.Library(libPath, {
    'Initialize': ['void', []],
    'Stop': ['void', []],
    'SendDogStatsDMetric': ['int32', ['pointer', 'size_t']],
    'GetMetrics': [MetricsData, []]
});

function sendMetric(metric) {
    const buffer = Buffer.from(metric, 'utf8');
    return libagent.SendDogStatsDMetric(buffer, buffer.length);
}

async function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

async function main() {
    console.log('DogStatsD Example - Sending Custom Metrics');
    console.log('==========================================\n');

    // Initialize libagent
    libagent.Initialize();
    console.log('Initialized libagent\n');

    // Give the agent a moment to start
    await sleep(1000);

    // Example 1: Counter - Page views
    console.log('Sending counter metric (page views)...');
    let result = sendMetric('example.page.views:1|c|#env:prod,service:web');
    console.log(`✓ Counter sent: ${result === 0}\n`);

    // Example 2: Gauge - Temperature
    console.log('Sending gauge metric (temperature)...');
    result = sendMetric('example.temperature:72.5|g|#location:office,sensor:A1');
    console.log(`✓ Gauge sent: ${result === 0}\n`);

    // Example 3: Histogram - Response time
    console.log('Sending histogram metric (response time)...');
    result = sendMetric('example.request.duration:250|h|@0.5|#endpoint:/api/users');
    console.log(`✓ Histogram sent: ${result === 0}\n`);

    // Example 4: Distribution - Response size
    console.log('Sending distribution metric (response size)...');
    result = sendMetric('example.response.size:1024|d|#status:200');
    console.log(`✓ Distribution sent: ${result === 0}\n`);

    // Example 5: Timing - Database query
    console.log('Sending timing metric (database query)...');
    result = sendMetric('example.db.query.time:45|ms|#db:postgres,table:users');
    console.log(`✓ Timing sent: ${result === 0}\n`);

    // Example 6: Set - Unique visitors
    console.log('Sending set metric (unique visitors)...');
    result = sendMetric('example.unique.visitors:user_12345|s');
    console.log(`✓ Set sent: ${result === 0}\n`);

    // Example 7: Batched metrics
    console.log('Sending batched metrics...');
    const batch = `example.requests.total:1|c|#env:prod
example.cache.hits:42|c|#cache:redis
example.active.connections:100|g
example.cpu.usage:75.5|g|#host:web01`;
    result = sendMetric(batch);
    console.log(`✓ Batched metrics sent (4 metrics): ${result === 0}\n`);

    // Note: GetMetrics would require proper struct parsing
    console.log('DogStatsD metrics sent successfully!\n');

    // Clean shutdown
    console.log('Stopping libagent...');
    libagent.Stop();
    console.log('Done!');
}

main().catch(console.error);
