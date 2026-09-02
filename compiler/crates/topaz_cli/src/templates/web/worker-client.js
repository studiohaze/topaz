function defaultWorkerUrl() {
  return new URL("./topaz-web-worker.js", import.meta.url);
}

function defaultWasmUrl() {
  return new URL("./topaz-web.wasm", import.meta.url);
}

function normalizeWasmSource(source) {
  return source instanceof URL ? source.href : source;
}

function makeWorker(workerOrUrl, workerOptions) {
  if (workerOrUrl instanceof Worker) {
    return workerOrUrl;
  }
  return new Worker(workerOrUrl || defaultWorkerUrl(), { type: "module", ...(workerOptions || {}) });
}

export function createTopazWorker(workerOrUrl, options = {}) {
  const worker = makeWorker(workerOrUrl, options.workerOptions);
  let nextId = 1;
  let exportNames = [];
  const pending = new Map();

  worker.onmessage = (event) => {
    const message = event.data || {};
    const slot = pending.get(message.id);
    if (!slot) return;
    pending.delete(message.id);
    if (message.status === "error") {
      slot.reject(new Error(message.message || "Topaz worker call failed"));
      return;
    }
    slot.resolve(message);
  };

  function request(message) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      worker.postMessage({ ...message, id });
    });
  }

  const ready = request({
    type: "init",
    wasm: normalizeWasmSource(options.wasm || defaultWasmUrl()),
  }).then((message) => {
    exportNames = message.exportNames || [];
    return exportNames.slice();
  });

  function callExportJson(name, argsJson = "[]") {
    return ready.then(() =>
      request({ type: "callJson", name, argsJson }).then((message) => message.result)
    );
  }

  function callExport(name, args = []) {
    return ready.then(() =>
      request({ type: "call", name, args }).then((message) => message.outcome)
    );
  }

  function callExportTraceJson(name, argsJson = "[]", input = "") {
    return ready.then(() =>
      request({ type: "callTraceJson", name, argsJson, input }).then((message) => message.result)
    );
  }

  function callExportTrace(name, args = [], input = "") {
    return ready.then(() =>
      request({ type: "callTrace", name, args, input }).then((message) => message.trace)
    );
  }

  const callableExports = new Proxy(Object.create(null), {
    get(_target, name) {
      if (typeof name !== "string") return undefined;
      return (...args) => callExport(name, args);
    },
  });

  function terminate() {
    for (const [, slot] of pending) {
      slot.reject(new Error("Topaz worker terminated"));
    }
    pending.clear();
    worker.terminate();
  }

  return {
    worker,
    ready,
    get exportNames() {
      return exportNames.slice();
    },
    exports: callableExports,
    callExportJson,
    callExport,
    callExportTraceJson,
    callExportTrace,
    terminate,
  };
}
