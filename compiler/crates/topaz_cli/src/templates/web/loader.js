const encoder = new TextEncoder();
const decoder = new TextDecoder();

async function instantiateSource(source, imports) {
  if (source instanceof WebAssembly.Module) {
    return WebAssembly.instantiate(source, imports);
  }
  if (source instanceof Response) {
    const bytes = await source.arrayBuffer();
    return WebAssembly.instantiate(bytes, imports);
  }
  if (typeof source === "string" || source instanceof URL) {
    const response = await fetch(source);
    const bytes = await response.arrayBuffer();
    return WebAssembly.instantiate(bytes, imports);
  }
  return WebAssembly.instantiate(source, imports);
}

export async function instantiateTopaz(source, imports = {}) {
  const result = await instantiateSource(source, imports);
  const instance = result.instance || result;
  const wasm = instance.exports;
  const memory = wasm.memory;

  function release(ptr, len) {
    if (ptr === 0 && len === 0) return;
    if (typeof wasm.topaz_free_checked === "function") {
      const status = wasm.topaz_free_checked(ptr, len);
      if (status !== 0) {
        const reason = status === 2 ? "allocation length mismatch" : "unknown allocation";
        throw new Error(`Topaz Web ABI free rejected: ${reason}`);
      }
      return;
    }
    wasm.topaz_free(ptr, len);
  }

  function readString(pair) {
    const packed = BigInt(pair);
    const ptr = Number(packed >> 32n);
    const len = Number(packed & 0xffffffffn);
    if (len === 0) return "";
    const bytes = new Uint8Array(memory.buffer, ptr, len);
    const text = decoder.decode(bytes);
    release(ptr, len);
    return text;
  }

  function writeString(text) {
    const bytes = encoder.encode(text);
    if (bytes.length === 0) return [0, 0];
    const ptr = wasm.topaz_alloc(bytes.length);
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  }

  function callExportJson(name, argsJson = "[]") {
    const [namePtr, nameLen] = writeString(name);
    const [argsPtr, argsLen] = writeString(argsJson);
    try {
      return readString(wasm.topaz_call_export_json(namePtr, nameLen, argsPtr, argsLen));
    } finally {
      release(namePtr, nameLen);
      release(argsPtr, argsLen);
    }
  }

  function callExport(name, args = []) {
    return JSON.parse(callExportJson(name, JSON.stringify(args)));
  }

  function callExportTraceJson(name, argsJson = "[]", input = "") {
    const [namePtr, nameLen] = writeString(name);
    const [argsPtr, argsLen] = writeString(argsJson);
    const [inputPtr, inputLen] = writeString(input);
    try {
      return readString(
        wasm.topaz_call_export_trace_json(namePtr, nameLen, argsPtr, argsLen, inputPtr, inputLen)
      );
    } finally {
      release(namePtr, nameLen);
      release(argsPtr, argsLen);
      release(inputPtr, inputLen);
    }
  }

  function callExportTrace(name, args = [], input = "") {
    return JSON.parse(callExportTraceJson(name, JSON.stringify(args), input));
  }

  const exportNames = JSON.parse(readString(wasm.topaz_export_names_json()));
  const callableExports = Object.create(null);
  for (const exportName of exportNames) {
    callableExports[exportName] = (...args) => callExport(exportName, args);
  }

  return {
    instance,
    exportNames,
    exports: callableExports,
    callExportJson,
    callExport,
    callExportTraceJson,
    callExportTrace,
  };
}
