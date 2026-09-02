import { TOPAZ_TOOLCHAIN_VERSION, instantiateTopaz } from "./topaz-web.js";

const EXPECTED_TOOLCHAIN_VERSION = "__TOPAZ_TOOLCHAIN_VERSION__";
const WEB_LIFECYCLE = "__TOPAZ_WEB_LIFECYCLE__";
const WEB_CAPABILITIES = Object.freeze({
  openText: __TOPAZ_OPEN_TEXT__,
  downloadText: __TOPAZ_DOWNLOAD_TEXT__,
  localState: __TOPAZ_LOCAL_STATE__,
});
const STATE_SCHEMA = "topaz.web-state.v1";
const STATE_NAMESPACE = "__TOPAZ_STATE_NAMESPACE__";
const MAX_TEXT_BYTES = __TOPAZ_MAX_TEXT_BYTES__;
const MAX_LIVE_REQUESTS = __TOPAZ_MAX_LIVE_REQUESTS__;
const MAX_STATE_VALUE_BYTES = __TOPAZ_MAX_STATE_VALUE_BYTES__;
const MAX_STATE_KEYS = __TOPAZ_MAX_STATE_KEYS__;
const MAX_REQUEST_ID_BYTES = 128;
const MAX_ACCEPT_BYTES = 256;
const MAX_FILENAME_BYTES = 255;
const MAX_MEDIA_TYPE_BYTES = 128;

const mount = document.getElementById("topaz-app");
const TAGS = new Set(["a", "article", "aside", "blockquote", "button", "code", "del", "div", "em", "footer", "form", "h1", "h2", "h3", "header", "hr", "img", "input", "label", "li", "main", "nav", "ol", "option", "p", "pre", "section", "select", "small", "span", "strong", "table", "tbody", "td", "textarea", "th", "thead", "tr", "ul"]);
const EVENTS = new Set(["change", "click", "input", "keydown", "keyup", "submit"]);
const ATTRS = new Set(["alt", "aria-label", "aria-live", "checked", "class", "disabled", "for", "height", "href", "id", "max", "maxlength", "min", "minlength", "name", "placeholder", "role", "selected", "src", "step", "title", "type", "value", "width"]);
const MAX_NODES = 10000;
const MAX_DEPTH = 128;
const MAX_COMMANDS = 100;
let stopped = false;
let nodeCount = 0;
let renderIds = new Set();
let dispatchBudget = MAX_COMMANDS;
const liveRequests = new Map();
const utf8 = new TextEncoder();

function fail(error) {
  if (stopped) return;
  stopped = true;
  liveRequests.clear();
  mount.replaceChildren(document.createTextNode(`Topaz application stopped: ${error instanceof Error ? error.message : String(error)}`));
}

function abiString(value) { return { $: "string", value }; }
function abiInt(value) { return { $: "int", value: String(value) }; }
function abiBool(value) { return { $: "bool", value }; }
function abiOption(value) { return value === null ? { $: "none" } : { $: "some", value }; }
const ENUM_INDICES = Object.freeze({
  "LocalDataResult.TextOpened": "0",
  "LocalDataResult.DownloadStarted": "1",
  "LocalDataResult.Cancelled": "2",
  "LocalDataResult.Failed": "3",
  "LocalStateResult.Loaded": "0",
  "LocalStateResult.Saved": "1",
  "LocalStateResult.Deleted": "2",
  "LocalStateResult.Failed": "3",
  "WebAppEvent.Browser": "0",
  "WebAppEvent.LocalData": "1",
  "WebAppEvent.LocalState": "2",
});
function abiEnum(id, variant, payloads = []) {
  const index = ENUM_INDICES[`${id}.${variant}`];
  if (index === undefined) throw new Error(`unknown host enum ${id}.${variant}`);
  return { $: "enum", id, variant, index, payloads };
}
function abiRecord(id, fields) {
  return { $: "nominal-record", id, fields: Object.entries(fields).map(([name, value]) => ({ name, value })) };
}

function nominalIdIs(value, expected) {
  return Boolean(
    value
    && typeof value.id === "string"
    && (value.id === expected || value.id.endsWith(`::${expected}`))
  );
}

function field(value, name) {
  if (!value || value.$ !== "nominal-record" || !Array.isArray(value.fields)) {
    const kind = value && typeof value.$ === "string" ? value.$ : typeof value;
    const id = value && typeof value.id === "string" ? ` ${value.id}` : "";
    throw new Error(`invalid nominal record for ${name}: ${kind}${id}`);
  }
  const found = value.fields.find((entry) => entry && entry.name === name);
  if (!found) throw new Error(`missing field ${name}`);
  return found.value;
}

function stringValue(value) {
  if (!value || value.$ !== "string" || typeof value.value !== "string") throw new Error("expected string value");
  return value.value;
}

function option(value) {
  if (value && value.$ === "none") return null;
  if (value && value.$ === "some") return value.value;
  throw new Error("invalid option value");
}

function safeUrl(value) {
  const url = new URL(value, document.baseURI);
  if (!new Set(["http:", "https:"]).has(url.protocol)) throw new Error("unsafe URL protocol");
  return url.href;
}

function setSafeAttribute(element, name, value) {
  const lower = name.toLowerCase();
  if (lower.startsWith("on") || (!ATTRS.has(lower) && !lower.startsWith("aria-") && !lower.startsWith("data-"))) throw new Error(`unsafe attribute ${name}`);
  if (lower === "href" || lower === "src") value = safeUrl(value);
  if (lower === "id") {
    if (!/^[A-Za-z][A-Za-z0-9_-]*$/.test(value) || renderIds.has(value)) throw new Error(`duplicate or unstable id ${value}`);
    renderIds.add(value);
  }
  element.setAttribute(lower, value);
  if (lower === "value" && element instanceof HTMLTextAreaElement) element.value = value;
}

function browserEvent(event) {
  const target = event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement ? event.target : null;
  const optional = (value) => value === null ? { $: "none" } : { $: "some", value };
  return {
    $: "nominal-record",
    id: "BrowserEvent",
    fields: [
      { name: "kind", value: { $: "string", value: event.type } },
      { name: "targetId", value: optional(event.target instanceof Element && event.target.id ? { $: "string", value: event.target.id } : null) },
      { name: "value", value: optional(target ? { $: "string", value: target.value } : null) },
      { name: "checked", value: optional(event.target instanceof HTMLInputElement ? { $: "bool", value: event.target.checked } : null) },
      { name: "key", value: optional(event instanceof KeyboardEvent ? { $: "string", value: event.key } : null) },
    ],
  };
}

function renderHtml(value, depth = 0) {
  if (++nodeCount > MAX_NODES || depth > MAX_DEPTH) throw new Error("render budget exhausted");
  if (!value || value.$ !== "enum" || !nominalIdIs(value, "Html") || !Array.isArray(value.payloads)) throw new Error("invalid Html value");
  if (value.variant === "Text" && value.payloads.length === 1) return document.createTextNode(stringValue(value.payloads[0]));
  if (value.variant !== "Element" || value.payloads.length !== 1) throw new Error("invalid Html variant");
  const spec = value.payloads[0];
  const tag = stringValue(field(spec, "tag")).toLowerCase();
  if (!TAGS.has(tag)) throw new Error(`unsafe element ${tag}`);
  const element = document.createElement(tag);
  const attrs = field(spec, "attrs");
  if (!attrs || attrs.$ !== "array" || !Array.isArray(attrs.items)) throw new Error("invalid attributes");
  for (const attr of attrs.items) setSafeAttribute(element, stringValue(field(attr, "name")), stringValue(field(attr, "value")));
  const events = field(spec, "events");
  if (!events || events.$ !== "array" || !Array.isArray(events.items)) throw new Error("invalid events");
  for (const binding of events.items) {
    const name = stringValue(field(binding, "name")).toLowerCase();
    if (!EVENTS.has(name)) throw new Error(`unsupported event ${name}`);
    const message = field(binding, "message");
    element.addEventListener(name, (event) => {
      if (name === "submit") event.preventDefault();
      stepBrowser(message, browserEvent(event));
    });
  }
  const children = field(spec, "children");
  if (!children || children.$ !== "array" || !Array.isArray(children.items)) throw new Error("invalid children");
  for (const child of children.items) element.append(renderHtml(child, depth + 1));
  return element;
}

function stableSelector(value) {
  const selector = stringValue(value);
  if (!/^#[A-Za-z][A-Za-z0-9_-]*$/.test(selector)) throw new Error("commands require a stable #id selector");
  return selector;
}

function utf8Length(value) { return utf8.encode(value).length; }

function requestIdValue(value) {
  const id = stringValue(value);
  if (!/^[A-Za-z][A-Za-z0-9._:-]*$/.test(id) || utf8Length(id) > MAX_REQUEST_ID_BYTES) {
    throw new Error("local-data request id is invalid or too long");
  }
  return id;
}

function stateKeyValue(value) {
  const key = stringValue(value);
  if (!/^[A-Za-z][A-Za-z0-9._:-]*$/.test(key) || utf8Length(key) > MAX_REQUEST_ID_BYTES) {
    throw new Error("local-state key is invalid or too long");
  }
  return key;
}

function acceptValue(value) {
  const accept = stringValue(value);
  if (utf8Length(accept) > MAX_ACCEPT_BYTES) throw new Error("file accept hint is too long");
  if (accept === "") return accept;
  const tokens = accept.split(",").map((token) => token.trim());
  const extension = /^\.[A-Za-z0-9][A-Za-z0-9._+-]{0,31}$/;
  const mediaType = /^[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]{0,63}\/(?:\*|[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]{0,63})$/;
  if (tokens.some((token) => !extension.test(token) && !mediaType.test(token))) {
    throw new Error("file accept hint contains an unsupported token");
  }
  return tokens.join(",");
}

function filenameValue(value) {
  const filename = stringValue(value);
  if (filename === "" || filename === "." || filename === ".." || utf8Length(filename) > MAX_FILENAME_BYTES || /[\\/\u0000-\u001f\u007f]/.test(filename)) {
    throw new Error("download filename is invalid or too long");
  }
  return filename;
}

function openedMetadata(file) {
  const name = String(file.name || "");
  const mediaType = String(file.type || "");
  if (name === "" || utf8Length(name) > MAX_FILENAME_BYTES || /[\\/\u0000-\u001f\u007f]/.test(name)) {
    return { error: localFailure("file-read-failed", "selected file name is invalid or too long") };
  }
  if (utf8Length(mediaType) > MAX_MEDIA_TYPE_BYTES || /[\u0000-\u001f\u007f]/.test(mediaType)) {
    return { error: localFailure("file-read-failed", "selected file media type is invalid or too long") };
  }
  return { name, mediaType };
}

function mediaTypeValue(value) {
  const mediaType = stringValue(value);
  if (utf8Length(mediaType) > MAX_MEDIA_TYPE_BYTES || !/^(?:text\/[A-Za-z0-9!#$&^_.+-]+|application\/json)(?:;\s*charset=utf-8)?$/i.test(mediaType)) {
    throw new Error("download media type is invalid or unsupported");
  }
  return mediaType;
}

function localFailure(code, message) {
  return abiEnum("LocalDataResult", "Failed", [abiString(code), abiString(String(message).slice(0, 512))]);
}

function stateFailure(code, message) {
  return abiEnum("LocalStateResult", "Failed", [abiString(code), abiString(String(message).slice(0, 512))]);
}

function storageFailure(error) {
  if (error && error.name === "SecurityError") return stateFailure("state-denied", "browser storage access was denied");
  if (error && error.name === "QuotaExceededError") return stateFailure("state-quota", "browser storage quota was exceeded");
  return stateFailure("state-unavailable", "browser storage is unavailable");
}

function reserveRequest(id, message) {
  if (liveRequests.has(id)) throw new Error(`duplicate live local request id ${id}`);
  if (liveRequests.size >= MAX_LIVE_REQUESTS) throw new Error("live local request budget exhausted");
  liveRequests.set(id, message);
}

function completeLocalState(id, result) {
  if (stopped) return;
  if (!liveRequests.has(id)) {
    fail(new Error(`unknown or stale local-state completion ${id}`));
    return;
  }
  const message = liveRequests.get(id);
  liveRequests.delete(id);
  const event = abiEnum("WebAppEvent", "LocalState", [
    abiRecord("LocalStateEvent", { requestId: abiString(id), result }),
  ]);
  stepLocal(message, event);
}

function stateEnvelope(value) {
  return JSON.stringify({ schema: STATE_SCHEMA, value });
}

function decodedState(raw) {
  try {
    const parsed = JSON.parse(raw);
    const keys = parsed && typeof parsed === "object" && !Array.isArray(parsed) ? Object.keys(parsed).sort() : [];
    if (keys.join(",") !== "schema,value" || parsed.schema !== STATE_SCHEMA || typeof parsed.value !== "string" || utf8Length(parsed.value) > MAX_STATE_VALUE_BYTES) return null;
    return parsed.value;
  } catch (_) {
    return null;
  }
}

function namespacedKeyCount(store) {
  let count = 0;
  for (let index = 0; index < store.length; index += 1) {
    const key = store.key(index);
    if (typeof key === "string" && key.startsWith(STATE_NAMESPACE)) count += 1;
  }
  return count;
}

function startLoadState(args) {
  if (!WEB_CAPABILITIES.localState) throw new Error("LoadState requires [capabilities.web].local_state = true");
  if (args.length !== 3) throw new Error("invalid LoadState command");
  const id = requestIdValue(args[0]);
  const key = stateKeyValue(args[1]);
  reserveRequest(id, args[2]);
  let result;
  try {
    const raw = window.localStorage.getItem(`${STATE_NAMESPACE}${key}`);
    if (raw === null) result = abiEnum("LocalStateResult", "Loaded", [abiString(key), abiOption(null)]);
    else {
      const value = decodedState(raw);
      result = value === null
        ? stateFailure("state-corrupt", "stored state has an invalid envelope")
        : abiEnum("LocalStateResult", "Loaded", [abiString(key), abiOption(abiString(value))]);
    }
  } catch (error) {
    result = storageFailure(error);
  }
  queueMicrotask(() => completeLocalState(id, result));
}

function startSaveState(args) {
  if (!WEB_CAPABILITIES.localState) throw new Error("SaveState requires [capabilities.web].local_state = true");
  if (args.length !== 4) throw new Error("invalid SaveState command");
  const id = requestIdValue(args[0]);
  const key = stateKeyValue(args[1]);
  const value = stringValue(args[2]);
  reserveRequest(id, args[3]);
  if (utf8Length(value) > MAX_STATE_VALUE_BYTES) {
    queueMicrotask(() => completeLocalState(id, stateFailure("state-too-large", `state value exceeds ${MAX_STATE_VALUE_BYTES} bytes`)));
    return;
  }
  let result;
  try {
    const store = window.localStorage;
    const storageKey = `${STATE_NAMESPACE}${key}`;
    if (store.getItem(storageKey) === null && namespacedKeyCount(store) >= MAX_STATE_KEYS) {
      result = stateFailure("state-key-budget", `application state exceeds ${MAX_STATE_KEYS} keys`);
    } else {
      store.setItem(storageKey, stateEnvelope(value));
      result = abiEnum("LocalStateResult", "Saved", [abiString(key)]);
    }
  } catch (error) {
    result = storageFailure(error);
  }
  queueMicrotask(() => completeLocalState(id, result));
}

function startDeleteState(args) {
  if (!WEB_CAPABILITIES.localState) throw new Error("DeleteState requires [capabilities.web].local_state = true");
  if (args.length !== 3) throw new Error("invalid DeleteState command");
  const id = requestIdValue(args[0]);
  const key = stateKeyValue(args[1]);
  reserveRequest(id, args[2]);
  let result;
  try {
    const store = window.localStorage;
    const storageKey = `${STATE_NAMESPACE}${key}`;
    const existed = store.getItem(storageKey) !== null;
    store.removeItem(storageKey);
    result = abiEnum("LocalStateResult", "Deleted", [abiString(key), abiBool(existed)]);
  } catch (error) {
    result = storageFailure(error);
  }
  queueMicrotask(() => completeLocalState(id, result));
}

function completeLocalData(id, result) {
  if (stopped) return;
  if (!liveRequests.has(id)) {
    fail(new Error(`unknown or stale local-data completion ${id}`));
    return;
  }
  const message = liveRequests.get(id);
  liveRequests.delete(id);
  const event = abiEnum("WebAppEvent", "LocalData", [
    abiRecord("LocalDataEvent", { requestId: abiString(id), result }),
  ]);
  stepLocal(message, event);
}

function startOpenText(args) {
  if (!WEB_CAPABILITIES.openText) throw new Error("OpenText requires [capabilities.web].open_text = true");
  if (args.length !== 3) throw new Error("invalid OpenText command");
  const id = requestIdValue(args[0]);
  const accept = acceptValue(args[1]);
  reserveRequest(id, args[2]);
  const input = document.createElement("input");
  input.type = "file";
  input.accept = accept;
  input.hidden = true;
  input.setAttribute("aria-hidden", "true");
  document.body.append(input);
  let settled = false;
  const finish = (result) => {
    if (settled) return;
    settled = true;
    input.remove();
    queueMicrotask(() => completeLocalData(id, result));
  };
  input.addEventListener("cancel", () => finish(abiEnum("LocalDataResult", "Cancelled")), { once: true });
  input.addEventListener("change", async () => {
    const file = input.files && input.files[0];
    if (!file) {
      finish(abiEnum("LocalDataResult", "Cancelled"));
      return;
    }
    const metadata = openedMetadata(file);
    if (metadata.error) {
      finish(metadata.error);
      return;
    }
    if (file.size > MAX_TEXT_BYTES) {
      finish(localFailure("file-too-large", `selected file exceeds ${MAX_TEXT_BYTES} bytes`));
      return;
    }
    let bytes;
    try {
      bytes = new Uint8Array(await file.arrayBuffer());
    } catch (error) {
      finish(localFailure("file-read-failed", error));
      return;
    }
    if (bytes.byteLength > MAX_TEXT_BYTES) {
      finish(localFailure("file-too-large", `selected file exceeds ${MAX_TEXT_BYTES} bytes`));
      return;
    }
    let text;
    try {
      text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch (_) {
      finish(localFailure("invalid-utf8", "selected file is not valid UTF-8"));
      return;
    }
    const documentValue = abiRecord("TextDocument", {
      name: abiString(metadata.name),
      mediaType: abiString(metadata.mediaType),
      sizeBytes: abiInt(bytes.byteLength),
      text: abiString(text),
    });
    finish(abiEnum("LocalDataResult", "TextOpened", [documentValue]));
  }, { once: true });
  try {
    input.click();
  } catch (error) {
    finish(localFailure("file-read-failed", error));
  }
}

function startDownloadText(args) {
  if (!WEB_CAPABILITIES.downloadText) throw new Error("DownloadText requires [capabilities.web].download_text = true");
  if (args.length !== 5) throw new Error("invalid DownloadText command");
  const id = requestIdValue(args[0]);
  const filename = filenameValue(args[1]);
  const mediaType = mediaTypeValue(args[2]);
  const value = stringValue(args[3]);
  reserveRequest(id, args[4]);
  if (utf8Length(value) > MAX_TEXT_BYTES) {
    queueMicrotask(() => completeLocalData(id, localFailure("download-too-large", `download exceeds ${MAX_TEXT_BYTES} bytes`)));
    return;
  }
  try {
    const url = URL.createObjectURL(new Blob([value], { type: mediaType }));
    const link = document.createElement("a");
    link.href = url;
    link.download = filename;
    link.hidden = true;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 0);
    queueMicrotask(() => completeLocalData(id, abiEnum("LocalDataResult", "DownloadStarted", [abiString(filename)])));
  } catch (error) {
    queueMicrotask(() => completeLocalData(id, localFailure("download-failed", error)));
  }
}

function executeDomCommand(command) {
  if (!command || command.$ !== "enum" || !nominalIdIs(command, "Command")) throw new Error("invalid DOM command");
  const args = command.payloads || [];
  if (command.variant === "Dispatch" && args.length === 1) {
    if (--dispatchBudget < 0) throw new Error("dispatch budget exhausted");
    stepBrowser(args[0], null);
    return;
  }
  if (command.variant === "Navigate" && args.length === 1) { location.assign(safeUrl(stringValue(args[0]))); return; }
  const element = document.querySelector(stableSelector(args[0]));
  if (!element) return;
  if (command.variant === "SetText" && args.length === 2) element.textContent = stringValue(args[1]);
  else if (command.variant === "SetAttr" && args.length === 3) setSafeAttribute(element, stringValue(args[1]), stringValue(args[2]));
  else if (command.variant === "RemoveAttr" && args.length === 2) element.removeAttribute(stringValue(args[1]));
  else if (command.variant === "AddClass" && args.length === 2) element.classList.add(stringValue(args[1]));
  else if (command.variant === "RemoveClass" && args.length === 2) element.classList.remove(stringValue(args[1]));
  else if (command.variant === "Focus" && args.length === 1 && element instanceof HTMLElement) element.focus();
  else throw new Error(`invalid command ${command.variant}`);
}

function executeCommands(value) {
  if (!value || value.$ !== "array" || !Array.isArray(value.items) || value.items.length > MAX_COMMANDS) throw new Error("command budget exhausted");
  for (const command of value.items) {
    if (WEB_LIFECYCLE === "v1") {
      executeDomCommand(command);
      continue;
    }
    if (!command || command.$ !== "enum" || !nominalIdIs(command, "WebAppCommand")) throw new Error("invalid Web App v2 command");
    const args = command.payloads || [];
    if (command.variant === "Dom" && args.length === 1) executeDomCommand(args[0]);
    else if (command.variant === "OpenText") startOpenText(args);
    else if (command.variant === "DownloadText") startDownloadText(args);
    else if (command.variant === "LoadState") startLoadState(args);
    else if (command.variant === "SaveState") startSaveState(args);
    else if (command.variant === "DeleteState") startDeleteState(args);
    else throw new Error(`invalid Web App v2 command ${command.variant}`);
  }
}

let app;
let model;

function call(name, args = []) {
  const result = app.callExport(name, args);
  if (!result || result.status !== "ok") throw new Error(result?.message || `${name} failed`);
  return result.value;
}

function commit(stepValue) {
  const expectedStep = WEB_LIFECYCLE === "v2" ? "WebAppStep" : "AppStep";
  if (!stepValue || stepValue.$ !== "nominal-record" || !nominalIdIs(stepValue, expectedStep)) throw new Error(`invalid ${expectedStep} value`);
  model = field(stepValue, "model");
  const active = document.activeElement instanceof HTMLElement && document.activeElement.id ? document.activeElement.id : null;
  const selectionElement = document.activeElement instanceof HTMLInputElement || document.activeElement instanceof HTMLTextAreaElement ? document.activeElement : null;
  const selection = selectionElement && typeof selectionElement.selectionStart === "number" && typeof selectionElement.selectionEnd === "number" ? [selectionElement.selectionStart, selectionElement.selectionEnd] : null;
  nodeCount = 0;
  renderIds = new Set();
  mount.replaceChildren(renderHtml(call("view", [model])));
  if (active) {
    const restored = document.getElementById(active);
    if (restored instanceof HTMLElement) restored.focus();
    if (selection && (restored instanceof HTMLInputElement || restored instanceof HTMLTextAreaElement)) restored.setSelectionRange(selection[0], selection[1]);
  }
  executeCommands(field(stepValue, "commands"));
}

function dispatchBrowserEvent() {
  return abiRecord("BrowserEvent", {
    kind: abiString("dispatch"),
    targetId: { $: "none" },
    value: { $: "none" },
    checked: { $: "none" },
    key: { $: "none" },
  });
}

function stepBrowser(message, event) {
  if (stopped) return;
  try {
    if (event !== null) dispatchBudget = MAX_COMMANDS;
    const boundedEvent = event || dispatchBrowserEvent();
    const updateEvent = WEB_LIFECYCLE === "v2"
      ? abiEnum("WebAppEvent", "Browser", [boundedEvent])
      : boundedEvent;
    commit(call("update", [model, message, updateEvent]));
  } catch (error) { fail(error); }
}

function stepLocal(message, event) {
  if (stopped) return;
  try {
    if (WEB_LIFECYCLE !== "v2") throw new Error("local completion requires Web lifecycle v2");
    dispatchBudget = MAX_COMMANDS;
    commit(call("update", [model, message, event]));
  } catch (error) { fail(error); }
}

try {
  if (TOPAZ_TOOLCHAIN_VERSION !== EXPECTED_TOOLCHAIN_VERSION) {
    throw new Error(`Topaz Web host/runtime version mismatch: expected ${EXPECTED_TOOLCHAIN_VERSION}, got ${TOPAZ_TOOLCHAIN_VERSION}`);
  }
  if (WEB_LIFECYCLE !== "v1" && WEB_LIFECYCLE !== "v2") {
    throw new Error(`unsupported Topaz Web lifecycle ${WEB_LIFECYCLE}`);
  }
  app = await instantiateTopaz(new URL("./topaz-web.wasm", import.meta.url));
  dispatchBudget = MAX_COMMANDS;
  commit(call("init"));
} catch (error) { fail(error); }

async function connectDevReload() {
  try {
    const first = await fetch("/__topaz_version", { cache: "no-store" });
    if (!first.ok) return;
    let version = await first.text();
    setInterval(async () => {
      try {
        const response = await fetch("/__topaz_version", { cache: "no-store" });
        if (!response.ok) return;
        const next = await response.text();
        if (next !== version) location.reload();
      } catch (_) {}
    }, 600);
  } catch (_) {}
}
connectDevReload();
