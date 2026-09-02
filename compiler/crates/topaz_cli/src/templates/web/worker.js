import { instantiateTopaz } from "./topaz-web.js";

let topazPromise = null;

function wasmSource(source) {
  if (source == null) {
    return new URL("./topaz-web.wasm", import.meta.url);
  }
  if (typeof source === "string") {
    return new URL(source, import.meta.url);
  }
  return source;
}

function messageError(error) {
  return error && typeof error.message === "string" ? error.message : String(error);
}

async function ensureTopaz(source) {
  if (topazPromise == null) {
    topazPromise = instantiateTopaz(wasmSource(source));
  }
  return topazPromise;
}

self.onmessage = async (event) => {
  const message = event.data || {};
  const id = message.id;
  try {
    if (message.type === "init") {
      const topaz = await ensureTopaz(message.wasm);
      self.postMessage({ id, status: "ready", exportNames: topaz.exportNames });
      return;
    }
    if (message.type === "call") {
      const topaz = await ensureTopaz();
      self.postMessage({
        id,
        status: "ok",
        outcome: topaz.callExport(message.name, message.args || []),
      });
      return;
    }
    if (message.type === "callJson") {
      const topaz = await ensureTopaz();
      self.postMessage({
        id,
        status: "ok",
        result: topaz.callExportJson(message.name, message.argsJson || "[]"),
      });
      return;
    }
    if (message.type === "callTrace") {
      const topaz = await ensureTopaz();
      self.postMessage({
        id,
        status: "ok",
        trace: topaz.callExportTrace(message.name, message.args || [], message.input || ""),
      });
      return;
    }
    if (message.type === "callTraceJson") {
      const topaz = await ensureTopaz();
      self.postMessage({
        id,
        status: "ok",
        result: topaz.callExportTraceJson(
          message.name,
          message.argsJson || "[]",
          message.input || ""
        ),
      });
      return;
    }
    self.postMessage({ id, status: "error", message: `unknown topaz worker message: ${message.type}` });
  } catch (error) {
    self.postMessage({ id, status: "error", message: messageError(error) });
  }
};
