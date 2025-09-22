// npm install ffi-napi ref-napi ref-struct-napi
const ffi = require('ffi-napi');
const ref = require('ref-napi');
const Struct = require('ref-struct-napi');

const size_t = ref.types.size_t;
const uint8Ptr = ref.refType(ref.types.uint8);
const charPtr = ref.refType(ref.types.char);
const voidPtr = ref.refType(ref.types.void);
const voidPtrPtr = ref.refType(voidPtr);

const LibagentHttpBuffer = Struct({
  data: uint8Ptr,
  len: size_t,
});

const LibagentHttpResponse = Struct({
  status: ref.types.uint16,
  headers: LibagentHttpBuffer,
  body: LibagentHttpBuffer,
});

const lib = ffi.Library('libagent', {
  'ProxyTraceAgentUds': ['int32', ['string', 'string', 'string', voidPtr, size_t, ref.refType(ref.refType(LibagentHttpResponse)), ref.refType(charPtr)]],
  'FreeHttpResponse': ['void', [ref.refType(LibagentHttpResponse)]],
  'FreeCString': ['void', [charPtr]],
});

function main() {
  const outRespPtr = ref.alloc(ref.refType(LibagentHttpResponse));
  const outErrPtr = ref.alloc(charPtr);

  const rc = lib.ProxyTraceAgentUds('GET', '/info', 'Accept: application/json\n', ref.NULL, 0, outRespPtr, outErrPtr);
  if (rc !== 0) {
    const errPtr = outErrPtr.deref();
    if (!errPtr.isNull()) {
      const msg = errPtr.readCString(0);
      console.error('error:', msg);
      lib.FreeCString(errPtr);
    } else {
      console.error('error rc=', rc);
    }
    return;
  }

  const respPtr = outRespPtr.deref();
  const resp = respPtr.deref();
  console.log('Status:', resp.status);

  const headersBuf = ref.reinterpret(resp.headers.data, resp.headers.len);
  console.log('Headers:\n' + headersBuf.toString('utf8'));
  const bodyBuf = ref.reinterpret(resp.body.data, resp.body.len);
  console.log('Body:\n' + bodyBuf.toString('utf8'));

  lib.FreeHttpResponse(respPtr);
}

main();

