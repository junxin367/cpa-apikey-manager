"use strict";
const API = "/v0/management/plugins/cpa-apikey-manager";
const $ = (id) => document.getElementById(id);
let token = "", snapshot = null, selected = "", draft = null, dirty = false;
// Unsaved drafts of keys other than the selected one, restored silently when switching back.
const drafts = new Map();
let view = "keys", toastTimer, priceTimer, dialogAction, busy = false;
let dialogTrigger = null, fieldSequence = 0, savingPolicy = false;
let priceRowsSignature = "";
let credentialSource = "";
let comboboxSequence = 0;
let savingEnforcement = false;
// Time zone setting: auto-detection runs once per connection when the server has no saved zone.
let savingTimeZone = false, autoTimeZoneDone = false, timeZonePicker = null;
let revealingKeys = false;
// "Show full keys" mode: survives saves and refreshes; cleared only by the user or a credential change.
let revealAll = false;
const revealedKeys = new Map();
let keySearchTimer, keySearchSequence = 0;
let remoteKeyMatches = new Set();
let priceFilter = "all";
const credentials = globalThis.CpaCredentials;
const credentialHost = window.location.host, credentialAgent = navigator.userAgent;
const PERIOD_LABELS = { day: "每天", week: "每周", month: "每月" };
const PERIOD_RESETS = { day: "每天 00:00 重置", week: "每周一 00:00 重置", month: "每月 1 日 00:00 重置" };

function icon(name) {
  const paths = {
    // Lucide icon paths (ISC license), 24x24 viewBox.
    shield: ["M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z", "m9 12 2 2 4-4"],
    key: ["M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z", "M16.5 7.5h.01"],
    coins: ["M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2Z", "M18.09 10.37A6 6 0 1 1 10.34 18", "M7 6h1v4", "m16.71 13.88.7.71-2.82 2.82"],
    models: ["M5 3h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z", "M15 3h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z", "M5 13h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2Z", "M15 13h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-4a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2Z"],
    clock: ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M12 6v6l4 2"],
    monitor: ["M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z", "M8 21h8", "M12 17v4"],
    refresh: ["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"],
    settings: ["M21 4h-7", "M10 4H3", "M21 12h-9", "M8 12H3", "M21 20h-5", "M12 20H3", "M14 2v4", "M8 10v4", "M16 18v4"],
    plus: ["M5 12h14", "M12 5v14"],
    close: ["M18 6 6 18", "m6 6 12 12"],
    info: ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M12 16v-4", "M12 8h.01"],
    alert: ["m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4", "M12 17h.01"],
    check: ["M20 6 9 17l-5-5"],
    eye: ["M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0", "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z"],
    eyeOff: ["M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49", "M14.084 14.158a3 3 0 0 1-4.242-4.242", "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143", "m2 2 20 20"],
    copy: ["M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2Z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"],
    search: ["M11 3a8 8 0 1 0 0 16 8 8 0 0 0 0-16Z", "m21 21-4.3-4.3"],
    pencil: ["M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z", "m15 5 4 4"],
    trash: ["M3 6h18", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2", "M10 11v6", "M14 11v6"],
    power: ["M12 2v10", "M18.4 6.6a9 9 0 1 1-12.77.04"],
    undo: ["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"],
    arrowRight: ["M5 12h14", "m12 5 7 7-7 7"],
    chevronDown: ["m6 9 6 6 6-6"],
  };
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24"); svg.setAttribute("class", "icon"); svg.setAttribute("aria-hidden", "true");
  for (const d of paths[name] || paths.info) {
    const path = document.createElementNS(svg.namespaceURI, "path"); path.setAttribute("d", d); svg.append(path);
  }
  return svg;
}
document.querySelectorAll("[data-icon]").forEach((node) => node.append(icon(node.dataset.icon)));
function pendingButton(button, message) {
  const children = [...button.childNodes], width = button.style.minWidth, disabled = button.disabled;
  button.style.minWidth = button.getBoundingClientRect().width + "px";
  button.disabled = true; button.setAttribute("aria-busy", "true"); button.classList.add("is-pending"); button.replaceChildren(icon("refresh"), message);
  return () => { button.replaceChildren(...children); button.style.minWidth = width; button.disabled = disabled; button.removeAttribute("aria-busy"); button.classList.remove("is-pending"); };
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key in node && !key.startsWith("aria")) node[key] = value;
    else node.setAttribute(key, value);
  }
  for (const child of children.flat()) if (child !== null && child !== undefined) node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  return node;
}
// Shared dropdown (listbox) used instead of native <select> so every option menu looks and behaves
// like the model combobox. Exposes `.value`, `.disabled` and an `.onchange` callback.
let dropdownSequence = 0;
function dropdown(options, value, label) {
  const listId = "dropdown-" + (++dropdownSequence);
  const text = el("span", { class: "dropdown-value" });
  const button = el("button", { class: "dropdown-trigger", type: "button", "aria-haspopup": "listbox", "aria-expanded": "false", "aria-controls": listId, "aria-label": label }, text, icon("chevronDown"));
  const menu = el("div", { id: listId, class: "combobox-menu dropdown-menu", role: "listbox", "aria-label": label, hidden: true });
  const root = el("div", { class: "dropdown" }, button, menu);
  let current = value, active = -1;
  const items = options.map(([optionValue, optionLabel], index) => el("button", {
    id: `${listId}-${index}`, class: "combobox-option dropdown-option", type: "button", role: "option", tabIndex: -1, "data-value": optionValue,
    onmouseenter: () => setActive(index), onclick: () => choose(index),
  }, el("span", {}, optionLabel), icon("check")));
  menu.append(...items);
  const indexOf = (optionValue) => options.findIndex(([candidate]) => candidate === optionValue);
  const sync = () => {
    const index = indexOf(current);
    text.textContent = index >= 0 ? options[index][1] : "";
    items.forEach((item, itemIndex) => item.setAttribute("aria-selected", String(itemIndex === index)));
  };
  const setActive = (index) => {
    active = (index + items.length) % items.length;
    items.forEach((item, itemIndex) => item.classList.toggle("active", itemIndex === active));
    button.setAttribute("aria-activedescendant", items[active].id);
    items[active].scrollIntoView?.({ block: "nearest" });
  };
  const open = () => {
    if (button.disabled) return;
    menu.hidden = false; button.setAttribute("aria-expanded", "true");
    fitMenu(root, menu);
    setActive(Math.max(0, indexOf(current)));
  };
  const close = () => {
    menu.hidden = true; button.setAttribute("aria-expanded", "false"); button.removeAttribute("aria-activedescendant");
  };
  const choose = (index) => {
    const next = options[index][0], changed = next !== current;
    current = next; sync(); close(); button.focus();
    if (changed) control.onchange?.(current);
  };
  button.onclick = () => (menu.hidden ? open() : close());
  button.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (menu.hidden) open(); else setActive(active + (event.key === "ArrowDown" ? 1 : -1));
    } else if ((event.key === "Enter" || event.key === " ") && !menu.hidden) {
      event.preventDefault(); choose(active);
    } else if (event.key === "Home" || event.key === "End") {
      if (!menu.hidden) { event.preventDefault(); setActive(event.key === "Home" ? 0 : items.length - 1); }
    } else if (event.key === "Escape" && !menu.hidden) {
      event.preventDefault(); event.stopPropagation(); close();
    } else if (event.key === "Tab") close();
  });
  root.addEventListener("focusout", (event) => { if (!root.contains(event.relatedTarget)) close(); });
  const control = {
    root, button, onchange: null,
    get value() { return current; },
    set value(next) { current = next; sync(); },
    get disabled() { return button.disabled; },
    set disabled(next) { button.disabled = next; if (next) close(); },
  };
  sync();
  return control;
}
// Inside a dialog, popup menus float over it (fixed to the viewport) so they are neither clipped by
// nor stretch the scrolling body; they open upward when there is more room above the control.
let placeFloatingMenu = null;
function fitMenu(root, menu) {
  const floating = !!root.closest("dialog");
  menu.classList.toggle("floating", floating);
  if (!floating) { menu.style.cssText = ""; return; }
  const place = () => {
    if (menu.hidden || !root.isConnected) { if (placeFloatingMenu === place) placeFloatingMenu = null; return; }
    const rect = root.getBoundingClientRect(), gap = 4, margin = 8;
    const below = innerHeight - rect.bottom - gap - margin, above = rect.top - gap - margin;
    const up = below < 200 && above > below;
    Object.assign(menu.style, {
      left: rect.left + "px", width: rect.width + "px",
      top: up ? "" : rect.bottom + gap + "px", bottom: up ? innerHeight - rect.top + gap + "px" : "",
      maxHeight: Math.max(96, Math.min(240, up ? above : below)) + "px",
    });
  };
  placeFloatingMenu = place;
  place();
}
addEventListener("resize", () => placeFloatingMenu?.());
document.addEventListener("scroll", (event) => {
  if (placeFloatingMenu && !event.target.closest?.(".combobox-menu")) placeFloatingMenu();
}, true);
// Allow/deny switch: checked = allow. Exposes `.value` ("allow" | "deny") like the select it replaces.
function accessSwitch(value, label, change, disabled = false) {
  const input = el("input", { type: "checkbox", role: "switch", "aria-label": label, checked: value !== "deny", disabled });
  const text = el("span", { class: "access-text" });
  const root = el("label", { class: "switch-control access-switch" }, input, text);
  const sync = () => { text.textContent = input.checked ? "允许" : "禁止"; root.classList.toggle("denied", !input.checked); };
  input.addEventListener("change", () => { sync(); change(input.checked ? "allow" : "deny"); });
  sync();
  return {
    root, input,
    get value() { return input.checked ? "allow" : "deny"; },
    set value(next) { input.checked = next !== "deny"; sync(); },
  };
}
function refocusAccess(label) {
  $("limit-rows").querySelector(`input[aria-label="${CSS.escape(label)}"]`)?.focus({ preventScroll: true });
}
function notify(message, error = false) {
  clearTimeout(toastTimer); $("toast").replaceChildren(icon(error ? "alert" : "check"), el("span", {}, message));
  ($("dialog").open ? $("dialog") : document.body).append($("toast"));
  // Keep the toast below the plugin header so it never covers navigation or header actions.
  const headerBottom = $("dialog").open ? 0 : document.querySelector(".plugin-header")?.getBoundingClientRect().bottom || 0;
  $("toast").style.top = Math.max(16, headerBottom + 12) + "px";
  $("toast").className = "toast" + (error ? " error" : ""); $("toast").hidden = false;
  toastTimer = setTimeout(() => $("toast").hidden = true, error ? 7000 : 3200);
}
async function api(path, method = "GET", body) {
  const response = await fetch(API + path, {
    method, credentials: "omit", cache: "no-store",
    headers: { ...(token ? { Authorization: "Bearer " + token } : {}), "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(20000),
  });
  let data;
  try { data = await response.json(); } catch { throw new Error("宿主返回了无效响应，请检查插件状态"); }
  if (!response.ok) {
    const error = new Error(data.error?.message || (response.status === 401 ? "管理密钥无效" : "请求失败：" + response.status));
    error.code = data.error?.code; error.status = response.status;
    if (response.status === 401) disconnectCredential(error.message);
    throw error;
  }
  return data;
}
async function guard(action) { try { await action(); } catch (error) { notify(error.message, true); } }
async function rawKey(keyId) {
  const data = await api("/key", "POST", { key_id: keyId });
  if (typeof data.key !== "string" || !data.key) throw new Error("宿主未返回完整密钥");
  return data.key;
}
async function copyText(value) {
  if (navigator.clipboard?.writeText) {
    try { await navigator.clipboard.writeText(value); return; } catch {}
  }
  const textarea = el("textarea", { value, readOnly: true, "aria-hidden": "true" });
  textarea.style.position = "fixed"; textarea.style.opacity = "0"; textarea.style.pointerEvents = "none";
  document.body.append(textarea); textarea.select();
  const copied = document.execCommand?.("copy");
  textarea.remove();
  if (!copied) throw new Error("浏览器未允许复制，请先查看完整密钥后手动复制");
}
function savedManagementKey() {
  return credentials?.readSavedManagementKey(localStorage, credentialHost, credentialAgent) || "";
}
function renderCredentialState(message = "") {
  // A key is only "connected" once plugin data has loaded; a stored key that failed to load can be retried.
  const connected = !!token && !!snapshot, failed = !!token && !snapshot;
  const labels = {
    host: "已自动复用宿主管理中心保存的管理密钥。",
    saved: "已使用本页保存在当前浏览器中的管理密钥。",
    temporary: "已使用当前页面的临时管理密钥，刷新后需要重新设置。",
  };
  $("credential-status").textContent = connected ? "已连接" : failed ? "连接失败" : "需要管理密钥";
  $("credential-status").className = "badge" + (connected ? "" : failed ? " danger" : " warn");
  $("credential-source").textContent = connected ? labels[credentialSource]
    : failed ? "已找到管理密钥，但未能读取插件数据。可点击顶部“刷新数据”重试，或重新输入。"
    : "未找到可用凭据，请在下方保存管理密钥。";
  $("credential-error").textContent = message;
  $("clear-credential").disabled = !savedManagementKey();
  $("refresh").disabled = !token;
  // Data views need a connected management key; keep them visibly unavailable until then.
  for (const node of document.querySelectorAll('[data-view="keys"], [data-view="prices"]')) {
    node.disabled = !connected; node.title = connected ? "" : "连接管理密钥后可用";
  }
  $("key-count").hidden = !connected || !snapshot;
  $("timezone-chip").hidden = !snapshot;
  renderSettingsInfo();
  renderTimezoneSetting();
  renderEnforcementState();
}
function renderSettingsInfo() {
  $("settings-info").hidden = !snapshot;
  if (!snapshot) return;
  const models = snapshot.models.filter((model) => !model.targets?.length), sync = snapshot.price_sync || {};
  const items = [
    ["插件版本", "v" + snapshot.version],
    ["结算时区", snapshot.timezone + (snapshot.timezone_source === "setting" ? "" : "（服务器）")],
    ["服务器时区", snapshot.server_timezone || snapshot.timezone],
    ["有效密钥", snapshot.keys.filter((key) => key.active).length + " 把"],
    ["模型价格", models.filter((model) => model.price).length + " / " + models.length + " 已设置"],
    ["价格自动补全", sync.enabled ? "每 " + sync.interval_hours + " 小时" : "已关闭"],
    ["插件开始记录", snapshot.recording_since ? date(snapshot.recording_since, true, false) : "—"],
  ];
  $("settings-info-list").replaceChildren(...items.map(([label, value]) => el("div", {}, el("dt", {}, label), el("dd", {}, value))));
}
function renderEnforcementState() {
  const connected = !!token && !!snapshot;
  const enabled = connected && snapshot.enforcement_enabled === true;
  $("plugin-status").textContent = connected ? (enabled ? "拦截已启用" : "拦截未启用") : "状态待连接";
  $("plugin-status-dot").classList.toggle("disabled", !enabled);
  $("plugin-status-chip").classList.toggle("warn", !enabled);
  const control = $("settings-enforcement-enabled");
  control.checked = enabled;
  control.disabled = !connected || savingEnforcement;
  $("settings-enforcement-label").textContent = enabled ? "已启用" : "未启用";
  $("settings-enforcement-detail").textContent = !connected
    ? "连接管理接口后可设置。"
    : enabled ? "已配置限制的密钥会执行权限、额度与记账；无规则密钥直接放行。" : "当前请求直接放行，不检查权限、不消耗额度。";
  if (!snapshot) {
    $("system-error").hidden = true;
    return;
  }
  const problem = snapshot.source_error || snapshot.health_error;
  const message = !enabled
    ? "权限与额度拦截尚未启用。可在“设置”中开启；开启前，当前请求会直接放行且不记账。"
    : problem ? "插件运行状态异常；需要进入处理流程的请求将采用保护性拦截：" + problem : "";
  $("system-error").className = "banner " + (!enabled ? "warning" : "error");
  $("system-error-text").textContent = message;
  $("system-error-action").hidden = enabled || view === "settings";
  $("system-error").hidden = !message;
}
function setView(next) {
  if (next !== "settings" && !snapshot) next = "settings";
  view = next;
  const viewName = ({ keys: "API 密钥", prices: "模型价格", settings: "设置" })[view];
  document.querySelectorAll("[data-view]").forEach((node) => {
    node.classList.toggle("active", node.dataset.view === view);
    if (node.dataset.view === view) node.setAttribute("aria-current", "page"); else node.removeAttribute("aria-current");
  });
  for (const name of ["keys", "prices", "settings"]) $(name + "-view").hidden = name !== view;
  $("app").classList.toggle("keys-active", view === "keys");
  $("breadcrumb").textContent = viewName;
  document.title = `${viewName} · CPA 密钥权限与额度`;
  schedulePriceRefresh();
  if (view === "keys" && snapshot) renderDetail();
  if (view === "settings") { renderCredentialState(); autoDetectTimeZone(); }
  else renderEnforcementState();
}
function disconnectCredential(message = "") {
  token = ""; credentialSource = ""; snapshot = null; selected = ""; draft = null; dirty = false; drafts.clear();
  revealedKeys.clear(); revealingKeys = false; revealAll = false;
  autoTimeZoneDone = false;
  clearTimeout(keySearchTimer); remoteKeyMatches.clear(); keySearchSequence++;
  clearTimeout(priceTimer);
  $("timezone-chip").hidden = true;
  setView("settings");
  renderCredentialState(message);
}
function modal(title, contents, action, button = "确认", iconName = "info") {
  $("dialog-title").textContent = title; $("dialog-body").replaceChildren(...contents);
  $("dialog-icon").replaceChildren(icon(iconName));
  $("dialog-icon").className = "dialog-icon" + (iconName === "alert" ? " warn" : "");
  $("dialog-error").textContent = ""; $("dialog-confirm").textContent = button;
  $("dialog-confirm").disabled = false; dialogAction = action;
  dialogTrigger = document.activeElement;
  document.body.classList.add("modal-open"); $("dialog").showModal();
}
function closeDialog() {
  if (busy) return;
  $("dialog").close();
}
$("dialog-close").onclick = closeDialog; $("dialog-cancel").onclick = closeDialog;
$("dialog").addEventListener("cancel", (event) => { event.preventDefault(); closeDialog(); });
$("dialog").addEventListener("close", () => {
  document.body.classList.remove("modal-open"); document.body.append($("toast"));
  if (dialogTrigger?.isConnected) dialogTrigger.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.model) [...$("price-rows").querySelectorAll("button")].find((node) => node.dataset.model === dialogTrigger.dataset.model)?.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.quotaModel) [...$("limit-rows").querySelectorAll("button")].find((node) => node.dataset.quotaModel === dialogTrigger.dataset.quotaModel)?.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.quotaChannel) [...$("limit-rows").querySelectorAll("button")].find((node) => node.dataset.quotaChannel === dialogTrigger.dataset.quotaChannel)?.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.quotaTotal) $("limit-rows").querySelector("[data-quota-total]")?.focus({ preventScroll: true });
});
$("dialog-form").onsubmit = async (event) => {
  event.preventDefault(); if (busy) return;
  if (![...$("dialog-body").querySelectorAll("input[data-unit]")].filter((input) => !input.disabled).map(validateNumber).every(Boolean)) return;
  busy = true; const restore = pendingButton($("dialog-confirm"), "处理中…");
  try { await dialogAction(); $("dialog").close(); }
  catch (error) { $("dialog-error").textContent = error.message; }
  finally { busy = false; restore(); }
};
function switchKey(keyId) {
  if (dirty && draft) drafts.set(selected, draft);
  selected = keyId;
  const saved = drafts.get(keyId); drafts.delete(keyId);
  draft = saved || structuredClone(chosen().policy); dirty = !!saved;
}
function field(label, input) { return el("label", {}, label, input); }
function modelCombobox(values) {
  const sequence = ++comboboxSequence, inputId = `model-combobox-${sequence}`, listId = `${inputId}-list`;
  const input = el("input", {
    id: inputId, placeholder: "输入或选择实际模型 ID", maxLength: 256, autocomplete: "off",
    role: "combobox", "aria-label": "实际模型", "aria-autocomplete": "list", "aria-controls": listId, "aria-expanded": "false",
  });
  const menu = el("div", { id: listId, class: "combobox-menu dropdown-menu", role: "listbox", hidden: true });
  const toggle = el("button", { class: "combobox-toggle", type: "button", "aria-label": "显示模型列表", "aria-controls": listId }, icon("chevronDown"));
  const root = el("div", { class: "model-combobox" }, input, toggle, menu);
  const allValues = [...new Set(values)].sort((left, right) => left.localeCompare(right));
  let options = [], activeIndex = -1;
  const setActive = (index) => {
    activeIndex = !options.length || index === -1 ? -1 : (index + options.length) % options.length;
    options.forEach((option, optionIndex) => option.classList.toggle("active", optionIndex === activeIndex));
    if (activeIndex >= 0) {
      input.setAttribute("aria-activedescendant", options[activeIndex].id);
      options[activeIndex].scrollIntoView?.({ block: "nearest" });
    } else input.removeAttribute("aria-activedescendant");
  };
  const choose = (value) => {
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.focus();
    close();
  };
  // Typing filters the list; opening it on an already chosen model lists everything, like a dropdown.
  const renderOptions = (filter) => {
    const text = input.value.trim(), query = filter || !allValues.includes(text) ? text.toLowerCase() : "";
    const filtered = allValues.filter((value) => value.toLowerCase().includes(query))
      .sort((left, right) => Number(right.toLowerCase().startsWith(query)) - Number(left.toLowerCase().startsWith(query)) || left.localeCompare(right));
    const visible = filtered.slice(0, 60);
    options = visible.map((value, index) => el("button", {
      id: `${listId}-option-${index}`, class: "combobox-option dropdown-option", type: "button", role: "option", tabIndex: -1,
      title: value, "data-value": value, "aria-selected": String(value === text),
      onmouseenter: () => setActive(index), onclick: () => choose(value),
    }, el("span", {}, value), icon("check")));
    const status = !filtered.length
      ? el("div", { class: "combobox-empty" }, query ? "没有匹配模型，可直接使用当前输入。" : "暂无可选择模型，可直接输入模型 ID。")
      : filtered.length > visible.length
        ? el("div", { class: "combobox-more" }, `还有 ${filtered.length - visible.length} 个结果，请继续输入筛选。`)
        : null;
    menu.replaceChildren(...options, ...(status ? [status] : []));
    setActive(query ? -1 : visible.indexOf(text));
  };
  const open = (filter = false) => {
    renderOptions(filter);
    menu.hidden = false;
    fitMenu(root, menu);
    input.setAttribute("aria-expanded", "true");
    toggle.setAttribute("aria-label", "收起模型列表");
  };
  const close = () => {
    menu.hidden = true;
    input.setAttribute("aria-expanded", "false");
    input.removeAttribute("aria-activedescendant");
    toggle.setAttribute("aria-label", "显示模型列表");
    activeIndex = -1;
  };
  input.addEventListener("focus", () => open());
  input.addEventListener("click", () => { if (menu.hidden) open(); });
  input.addEventListener("input", () => open(true));
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (menu.hidden) open();
      setActive(activeIndex < 0 ? (event.key === "ArrowDown" ? 0 : options.length - 1) : activeIndex + (event.key === "ArrowDown" ? 1 : -1));
    } else if (event.key === "Enter" && !menu.hidden && activeIndex >= 0) {
      event.preventDefault();
      choose(options[activeIndex].dataset.value);
    } else if (event.key === "Escape" && !menu.hidden) {
      event.preventDefault(); event.stopPropagation(); close();
    } else if (event.key === "Tab") close();
  });
  toggle.onclick = () => {
    if (menu.hidden) { input.focus(); open(); } else { input.focus(); close(); }
  };
  root.addEventListener("focusout", (event) => { if (!root.contains(event.relatedTarget)) close(); });
  return { root, input, close };
}
function numericField(attrs, unit, optional = false) {
  const input = el("input", attrs), message = el("small", { class: "field-error", id: "field-error-" + (++fieldSequence), hidden: true });
  input.dataset.unit = unit; input.dataset.optional = String(optional);
  input.setAttribute("aria-describedby", message.id);
  input.addEventListener("blur", () => validateNumber(input));
  input.addEventListener("input", () => { if (input.getAttribute("aria-invalid") === "true") validateNumber(input); });
  return { input, message };
}
function validateNumber(input) {
  const value = input.value.trim(), unit = input.dataset.unit;
  const format = unit === "tokens" ? /^\d+$/ : /^\d+(?:\.\d{0,6})?$/;
  let problem = value === "" && input.dataset.optional === "true" ? "" : !format.test(value) ? (unit === "tokens" ? "请输入非负整数 Token。" : "请输入非负金额，最多 6 位小数。") : "";
  if (!problem && value) {
    const [whole, fraction = ""] = value.split(".");
    const scaled = unit === "tokens" ? BigInt(whole) : BigInt(whole) * 1000000n + BigInt(fraction.padEnd(6, "0"));
    if (scaled > 9000000000000000n) problem = "数值超出支持范围，请调低数值。";
  }
  input.setAttribute("aria-invalid", String(!!problem));
  const message = $(input.getAttribute("aria-describedby"));
  if (message) { message.textContent = problem; message.hidden = !problem; }
  return !problem;
}
function date(value, time = true, seconds = true) {
  const options = { year: "numeric", month: "2-digit", day: "2-digit", ...(time ? { hour: "2-digit", minute: "2-digit", ...(seconds ? { second: "2-digit" } : {}), hour12: false } : {}) };
  try { return new Intl.DateTimeFormat("zh-CN", { ...options, timeZone: snapshot.timezone }).format(new Date(value)); }
  catch { return new Intl.DateTimeFormat("zh-CN", { ...options, timeZone: "UTC" }).format(new Date(new Date(value).getTime() + (snapshot.server_offset_seconds || 0) * 1000)); }
}
function validTimeZone(name) {
  if (!name) return false;
  try { new Intl.DateTimeFormat("en-US", { timeZone: name }); return true; } catch { return false; }
}
function browserTimeZone() {
  try { const name = Intl.DateTimeFormat().resolvedOptions().timeZone; return validTimeZone(name) ? name : ""; } catch { return ""; }
}
const zoneOffsets = new Map();
// Current UTC offset label of an IANA zone, e.g. "UTC+08:00"; empty when the browser cannot resolve it.
function zoneOffset(name) {
  if (!zoneOffsets.has(name)) {
    let label = "";
    try {
      const part = new Intl.DateTimeFormat("en-US", { timeZone: name, timeZoneName: "longOffset" }).formatToParts(new Date())
        .find((item) => item.type === "timeZoneName")?.value || "";
      label = part === "GMT" ? "UTC+00:00" : part.replace("GMT", "UTC");
    } catch {}
    zoneOffsets.set(name, label);
  }
  return zoneOffsets.get(name);
}
let zoneNames = null;
function timeZones() {
  if (!zoneNames) {
    let names = [];
    try { names = Intl.supportedValuesOf("timeZone"); } catch {}
    zoneNames = [...new Set(["UTC", ...names])];
  }
  const extra = [browserTimeZone(), snapshot?.timezone].filter((name) => validTimeZone(name) && !zoneNames.includes(name));
  return [...new Set([...extra, ...zoneNames])];
}
// Searchable IANA time zone picker; styled like `dropdown` with a filter field at the top of the menu.
function timeZoneSelect(onchoose) {
  const listId = "timezone-options";
  const text = el("span", { class: "dropdown-value" });
  const button = el("button", { class: "dropdown-trigger", type: "button", "aria-haspopup": "listbox", "aria-expanded": "false", "aria-controls": listId, "aria-label": "结算时区" }, text, icon("chevronDown"));
  const search = el("input", { class: "timezone-search", type: "search", placeholder: "搜索时区，如 Shanghai、+08", autocomplete: "off", role: "combobox", "aria-label": "搜索时区", "aria-controls": listId, "aria-expanded": "true", "aria-autocomplete": "list" });
  const list = el("div", { id: listId, class: "timezone-options", role: "listbox", "aria-label": "时区" });
  const menu = el("div", { class: "combobox-menu dropdown-menu timezone-menu", hidden: true }, search, list);
  const root = el("div", { class: "dropdown timezone-dropdown" }, button, menu);
  let current = "", names = [], options = [], active = -1;
  const sync = () => {
    const offset = current ? zoneOffset(current) : "";
    text.replaceChildren(el("span", {}, current || "—"), offset ? el("span", { class: "timezone-offset" }, offset) : "");
    button.title = current ? current + (offset ? "（" + offset + "）" : "") : "";
  };
  const setActive = (index) => {
    active = options.length ? (index + options.length) % options.length : -1;
    options.forEach((option, optionIndex) => option.classList.toggle("active", optionIndex === active));
    if (active >= 0) { search.setAttribute("aria-activedescendant", options[active].id); options[active].scrollIntoView?.({ block: "nearest" }); }
    else search.removeAttribute("aria-activedescendant");
  };
  const renderOptions = () => {
    const query = search.value.trim().toLowerCase();
    names = timeZones().filter((name) => !query || name.toLowerCase().replaceAll("_", " ").includes(query.replaceAll("_", " "))
      || zoneOffset(name).toLowerCase().includes(query));
    options = names.map((name, index) => el("button", {
      id: `${listId}-${index}`, class: "combobox-option dropdown-option", type: "button", role: "option", tabIndex: -1,
      "aria-selected": String(name === current), title: name,
      onmouseenter: () => setActive(index), onclick: () => choose(name),
    }, el("span", { class: "timezone-name" }, name), el("span", { class: "timezone-offset" }, zoneOffset(name)), icon("check")));
    list.replaceChildren(...options, ...(options.length ? [] : [el("div", { class: "combobox-empty" }, "没有匹配的时区")]));
    setActive(query ? 0 : Math.max(0, names.indexOf(current)));
  };
  const open = () => {
    if (button.disabled) return;
    zoneOffsets.clear(); search.value = "";
    menu.hidden = false; button.setAttribute("aria-expanded", "true");
    renderOptions(); search.focus();
  };
  const close = () => { menu.hidden = true; button.setAttribute("aria-expanded", "false"); };
  const choose = (name) => {
    close(); button.focus();
    if (name !== current) onchoose(name);
  };
  button.onclick = () => (menu.hidden ? open() : close());
  button.addEventListener("keydown", (event) => {
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && menu.hidden) { event.preventDefault(); open(); }
  });
  search.addEventListener("input", renderOptions);
  search.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setActive(active + (event.key === "ArrowDown" ? 1 : -1)); }
    else if (event.key === "Enter") { event.preventDefault(); if (active >= 0) choose(names[active]); }
    else if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); button.focus(); }
    else if (event.key === "Tab") close();
  });
  root.addEventListener("focusout", (event) => { if (!root.contains(event.relatedTarget)) close(); });
  sync();
  return {
    root, button,
    get value() { return current; },
    set value(next) { current = next || ""; sync(); },
    get disabled() { return button.disabled; },
    set disabled(next) { button.disabled = next; if (next) close(); },
  };
}
function renderTimezoneSetting() {
  const connected = !!token && !!snapshot;
  if (!timeZonePicker) {
    timeZonePicker = timeZoneSelect((name) => guard(() => saveTimeZone(name)));
    $("settings-timezone-control").append(timeZonePicker.root);
  }
  timeZonePicker.value = connected ? snapshot.timezone : "";
  timeZonePicker.disabled = !connected || savingTimeZone;
  const browser = browserTimeZone(), configured = connected && snapshot.timezone_source === "setting";
  const offset = connected ? zoneOffset(snapshot.timezone) : "";
  $("settings-timezone-detail").textContent = !connected
    ? "连接管理接口后可设置。"
    : savingTimeZone ? "正在保存时区…"
      : configured ? "已设置" + (offset ? "，当前 " + offset : "") + (browser === snapshot.timezone ? "，与浏览器一致。" : "。")
        : "尚未设置，暂用服务器时区" + (offset ? "（" + offset + "）" : "") + "。";
  const differs = connected && !!browser && browser !== snapshot.timezone;
  $("settings-timezone-browser-row").hidden = !differs;
  $("settings-timezone-browser-text").textContent = differs ? `当前浏览器时区：${browser}（${zoneOffset(browser)}）` : "";
  $("settings-timezone-browser").disabled = savingTimeZone;
}
async function saveTimeZone(name, automatic = false) {
  if (savingTimeZone || !token || !snapshot) return;
  savingTimeZone = true; renderTimezoneSetting();
  try {
    await api("/settings", "PUT", { timezone: name });
    savingTimeZone = false;
    await load(true);
    notify(automatic ? `已按浏览器自动识别时区：${snapshot.timezone}` : `结算时区已切换为 ${snapshot.timezone}`);
  } finally {
    savingTimeZone = false; renderTimezoneSetting();
  }
}
// Opening Settings adopts the browser's zone once when the server has never saved one.
function autoDetectTimeZone() {
  if (autoTimeZoneDone || savingTimeZone || !token || !snapshot || snapshot.timezone_source === "setting") return;
  const browser = browserTimeZone();
  if (!browser) return;
  autoTimeZoneDone = true;
  guard(() => saveTimeZone(browser, true));
}
function group(id) {
  return snapshot?.models.find((model) => model.id === id)?.channel_label || "其他";
}
function activeRules(policy) { return policy.rule_mode === "channel" ? policy.channel_rules : policy.rules; }
function modeLabel(mode) { return mode === "channel" ? "渠道" : "模型"; }
function chosen() { return snapshot?.keys.find((k) => k.id === selected); }
const formatAmount = (value) => Number(value).toLocaleString("zh-CN", { maximumFractionDigits: 6 });
const unitLabel = (unit) => unit === "money" ? "USD" : "Token";
function renderSaveState() {
  const key = chosen();
  $("save-policy").disabled = savingPolicy || !dirty || !key?.active; $("discard").disabled = !dirty;
}
function markDirty() {
  dirty = true; renderSaveState();
}
async function load(preserve = false) {
  const data = await api("/state");
  snapshot = data;
  if (!preserve) remoteKeyMatches.clear();
  pruneRevealedKeys();
  for (const keyId of drafts.keys()) if (!data.keys.some((key) => key.active && key.id === keyId)) drafts.delete(keyId);
  if (!data.keys.some((key) => key.id === selected)) selected = data.keys.find((k) => k.active)?.id || data.keys[0]?.id || "";
  // Keep the current unsaved draft across refreshes; saves reset `dirty` before reloading.
  if (!dirty || !draft || !chosen()?.active) { draft = chosen() ? structuredClone(chosen().policy) : null; dirty = false; }
  render();
  revealCard($("key-list").querySelector(".key-card.selected"));
  revealMissingKeys();
  if ($("key-search").value.trim()) scheduleKeySearch();
  if (view === "settings") autoDetectTimeZone();
}
function render() {
  $("version").textContent = "v" + snapshot.version; $("timezone").textContent = snapshot.timezone;
  $("timezone-chip").hidden = false;
  renderCredentialState();
  renderCounts();
  $("keys-unavailable").hidden = !snapshot.source_error;
  $("keys-content").hidden = !!snapshot.source_error;
  $("keys-summary").hidden = !!snapshot.source_error;
  $("keys-unavailable-reason").textContent = snapshot.source_error || "";
  renderKeys(); renderDetail(); renderPrices();
}
function renderCounts() {
  $("key-count").textContent = snapshot.keys.filter((k) => k.active).length;
  $("key-count").hidden = false;
  $("stat-keys").textContent = $("key-count").textContent;
  $("stat-models").textContent = snapshot.models.filter((model) => !model.targets?.length).length;
  $("stat-rules").textContent = snapshot.keys.reduce((count, key) => count + Object.keys(activeRules(key.policy)).length + (key.policy.total_quota_enabled ? 1 : 0), 0);
}
// Scroll only the key list (not the page) so a card is fully visible.
function revealCard(card) {
  const list = $("key-list");
  if (!card || list.offsetParent === null) return;
  const outer = list.getBoundingClientRect(), inner = card.getBoundingClientRect();
  if (inner.top < outer.top) list.scrollTop -= outer.top - inner.top + 4;
  else if (inner.bottom > outer.bottom) list.scrollTop += inner.bottom - outer.bottom + 4;
  if (inner.left < outer.left) list.scrollLeft -= outer.left - inner.left + 4;
  else if (inner.right > outer.right) list.scrollLeft += inner.right - outer.right + 4;
}
function renderRevealAllKeys() {
  const button = $("reveal-all-keys");
  const activeKeys = snapshot?.keys.filter((key) => key.active) || [];
  const allRevealed = revealAll && activeKeys.length > 0;
  button.disabled = revealingKeys || activeKeys.length === 0 || !!snapshot?.source_error;
  button.setAttribute("aria-pressed", String(allRevealed));
  button.setAttribute("aria-busy", String(revealingKeys));
  button.classList.toggle("is-pending", revealingKeys);
  button.replaceChildren(icon(revealingKeys ? "refresh" : allRevealed ? "eyeOff" : "eye"), revealingKeys ? "读取中…" : allRevealed ? "隐藏完整" : "查看完整");
}
function scheduleKeySearch() {
  clearTimeout(keySearchTimer);
  const query = $("key-search").value.trim();
  const sequence = ++keySearchSequence;
  remoteKeyMatches.clear();
  renderKeys();
  const activeKeys = snapshot?.keys.filter((key) => key.active) || [];
  if (!query || (activeKeys.length && activeKeys.every((key) => revealedKeys.has(key.id)))) return;
  keySearchTimer = setTimeout(async () => {
    try {
      const result = await api("/key-search", "POST", { query });
      if (sequence !== keySearchSequence) return;
      remoteKeyMatches = new Set(Array.isArray(result.key_ids) ? result.key_ids : []);
      renderKeys();
    } catch (error) {
      if (sequence === keySearchSequence) notify(error.message, true);
    }
  }, 180);
}
function renderKeys() {
  const search = $("key-search").value.trim().toLowerCase();
  const keys = snapshot.keys.filter((key) =>
    (key.masked + key.policy.note + (revealedKeys.get(key.id) || "")).toLowerCase().includes(search)
      || remoteKeyMatches.has(key.id));
  const focused = document.activeElement?.closest?.("#key-list .key-card")?.dataset.keyId;
  $("key-list").replaceChildren(...keys.map((key) => {
    const raw = revealedKeys.get(key.id), note = key.policy.note;
    const value = el("span", { class: "key-mask" + (raw ? " revealed" : ""), title: raw || key.masked }, raw || key.masked);
    const ruleCount = Object.keys(activeRules(key.policy)).length;
    const blocked = totalDenied(key.policy);
    const scopes = [...(key.policy.total_quota_enabled ? [blocked ? "全部模型禁止" : "总额度"] : []), ...(ruleCount ? ["按" + modeLabel(key.policy.rule_mode) + " " + ruleCount + " 条"] : [])];
    const meta = PERIOD_LABELS[key.policy.period] + " · " + (scopes.length ? scopes.join(" · ") : "未配置限制");
    const meter = keyMeter(key);
    const flags = [
      key.active ? null : el("span", { class: "badge danger" }, "已删除"),
      blocked ? el("span", { class: "badge danger" }, "已禁止使用") : null,
      meter?.state === "exhausted" ? el("span", { class: "badge danger" }, meter.label === "总额度" ? "总额度已耗尽" : "额度已耗尽") : null,
    ].filter(Boolean);
    const label = [note || key.masked, meta, ...(meter ? [meter.label + "已用 " + meter.percent.textContent] : []), ...flags.map((flag) => flag.textContent)].join("，");
    const copy = el("button", {
      class: "key-card-copy ghost icon-button", type: "button", disabled: !key.active, title: "复制完整密钥",
      "aria-label": `复制 ${note || key.masked} 的完整密钥`,
      onclick: () => guard(async () => {
        copy.disabled = true; copy.setAttribute("aria-busy", "true");
        try {
          await copyText(raw || await rawKey(key.id));
          copy.replaceChildren(icon("check")); copy.classList.add("copied");
          setTimeout(() => { copy.replaceChildren(icon("copy")); copy.classList.remove("copied"); }, 1500);
          notify("完整密钥已复制");
        } finally {
          copy.disabled = !key.active; copy.removeAttribute("aria-busy");
        }
      }),
    }, icon("copy"));
    const selectKey = el("button", {
      class: "key-card-select", type: "button",
      "aria-pressed": key.id === selected,
      "aria-label": "选择 " + label,
      onclick: () => {
      if (key.id === selected) return;
      switchKey(key.id); renderKeys(); renderDetail();
      document.querySelector(".detail-panel").scrollTop = 0;
      },
    }, el("strong", { class: "key-card-name key-card-line", title: note || raw || key.masked }, note || value),
    note ? el("span", { class: "key-card-line" }, value) : el("span", { class: "key-card-sub key-card-line" }, "未设置备注"),
    el("span", { class: "key-card-meta key-card-line", title: meta }, meta),
    // Fixed-height status row keeps every card the same height with or without a meter or badges.
    el("span", { class: "key-card-status" },
      meter ? el("span", { class: "key-card-meter", title: meter.description },
        el("span", { class: "key-card-meter-label" }, meter.label), meter.bar, meter.percent) : null,
      flags.length ? el("span", { class: "key-card-flags" }, flags) : null));
    return el("article", { class: "key-card" + (key.id === selected ? " selected" : "") + (key.active ? "" : " inactive"), role: "listitem", "data-key-id": key.id },
      selectKey, copy);
  }));
  if (!keys.length) $("key-list").append(el("div", { class: "empty compact" }, el("p", {}, search ? "没有匹配的密钥" : "宿主暂无 API 密钥，请先在宿主中创建。"), search ? el("button", { class: "secondary", onclick: clearKeySearch }, "清除搜索") : null));
  if (focused) $("key-list").querySelector(`[data-key-id="${CSS.escape(focused)}"] .key-card-select`)?.focus({ preventScroll: true });
  renderRevealAllKeys();
}
function clearKeySearch() {
  $("key-search").value = ""; $("clear-key-search").hidden = true;
  scheduleKeySearch();
}
// Usage meter for a quota: returns null when the usage cannot be compared with the limit.
function quotaMeter(used, limit) {
  const consumed = Number(used || 0), max = Number(limit);
  const ratio = max > 0 ? consumed / max : 1;
  const state = consumed >= max ? "exhausted" : ratio >= 0.8 ? "warning" : "";
  const percent = Math.min(100, ratio * 100);
  const fill = el("span"); fill.style.width = percent + "%";
  return {
    consumed, max, state, ratio,
    bar: el("span", { class: "progress" + (state ? " " + state : ""), role: "progressbar", "aria-label": "额度用量", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(Math.round(percent)) }, fill),
    percent: el("span", { class: "usage-percent" + (state ? " " + state : "") }, Math.min(999, Math.floor(ratio * 100)) + "%"),
  };
}
function keyMeter(key) {
  if (totalDenied(key.policy)) return null;
  const candidates = [];
  const add = (label, quota, usage) => {
    if (!quota || !usage || (quota.unit === "money" && usage.unpriced > 0)) return;
    const meter = quotaMeter(quota.unit === "money" ? usage.cost : usage.tokens, quota.limit);
    candidates.push({ ...meter, label, description: `${label}：${formatAmount(meter.consumed)} / ${formatAmount(meter.max)} ${unitLabel(quota.unit)}` });
  };
  if (key.policy.total_quota_enabled) add("总额度", key.policy.total_quota, key.total_usage);
  const channelMode = key.policy.rule_mode === "channel";
  for (const [id, rule] of Object.entries(activeRules(key.policy))) {
    if (rule.access === "deny") continue;
    add(channelMode ? snapshot.channels.find((channel) => channel.id === id)?.label || id : id,
      rule.quota, channelMode ? key.channel_usage?.[id] : key.quotas?.[id]);
  }
  // Different quota units cannot be summed; surface the active quota closest to its limit.
  return candidates.sort((left, right) => right.ratio - left.ratio)[0] || null;
}
// "All models" denied: the key is blocked while the total scope is switched on.
function totalDenied(policy) { return !!policy.total_quota_enabled && policy.total_access === "deny"; }
function renderDetail() {
  const key = chosen(); $("key-empty").hidden = !!key; $("key-detail").hidden = !key;
  if (!key) return;
  $("selected-key").textContent = key.policy.note || key.masked;
  $("selected-mask").textContent = key.policy.note ? key.masked : "未设置备注";
  $("selected-mask").classList.toggle("key-mask", !!key.policy.note);
  $("key-status").textContent = "已从宿主删除"; $("key-status").className = "badge danger"; $("key-status").hidden = key.active;
  $("key-note").value = draft.note; periodDropdown.value = draft.period;
  $("key-note").disabled = !key.active; periodDropdown.disabled = !key.active;
  $("period-note-text").textContent = PERIOD_RESETS[draft.period] +
    (draft.period === key.policy.period && key.period_reset_at ? "，下次 " + date(key.period_reset_at, true, false) : "，保存后按新周期计算") + "（" + snapshot.timezone + "）";
  renderSaveState();
  const configured = draft.total_quota_enabled || Object.keys(activeRules(draft)).length > 0;
  $("recording-note").textContent = snapshot.enforcement_enabled === false
    ? "拦截功能尚未启用；当前请求不会被插件拦截或记账。"
    : totalDenied(draft) ? "当前密钥已禁止使用全部模型，所有新请求都会被拒绝。"
    : !configured
      ? "当前密钥没有生效的限制配置，请求会直接放行且不记账。启用总额度或添加当前模式限制后开始处理。"
      : "当前密钥已配置限制；获准的生成请求会记账。密钥同步于 " + date(key.recording_since) + "，插件停用期间无法补算。" +
        (key.partial_period ? " 本周期更早时段仅包含已有记录。" : "");
  $("sync-models").disabled = !key.active;
  $("add-restriction").disabled = !key.active;
  $("empty-add-restriction").disabled = !key.active;
  renderLimits();
}
function renderLimits() {
  const key = chosen(), rows = [];
  $("total-quota-enabled").checked = draft.total_quota_enabled;
  $("total-quota-enabled").disabled = !key.active;
  if (draft.total_quota_enabled && (draft.total_quota || draft.total_access === "deny")) rows.push(totalRow());
  if (draft.rule_mode === "channel") {
    for (const id of Object.keys(draft.channel_rules).sort()) {
      const channel = snapshot.channels.find((item) => item.id === id);
      if (channel) rows.push(channelRow(channel));
    }
  } else {
    for (const model of Object.keys(draft.rules).sort()) rows.push(modelRow(model));
  }
  $("limit-rows").replaceChildren(...rows);
  $("limits-table-wrap").hidden = rows.length === 0;
  $("limits-empty").hidden = rows.length !== 0;
  const paused = draft.rule_mode === "channel" ? Object.keys(draft.rules).length : Object.keys(draft.channel_rules).length;
  const ruleCount = Object.keys(activeRules(draft)).length;
  $("limit-mode").hidden = !ruleCount;
  $("limit-mode").textContent = "按" + modeLabel(draft.rule_mode) + "限制 · " + ruleCount + " 条";
  const parts = [];
  if (!draft.total_quota_enabled && (draft.total_quota || draft.total_access === "deny"))
    parts.push((draft.total_access === "deny" ? "全部模型禁止设置" : "总额度配置") + "已保留，打开开关后才显示并生效");
  if (paused) parts.push("另有 " + paused + " 条" + (draft.rule_mode === "channel" ? "模型" : "渠道") + "限制已保留但暂停");
  $("limit-mode-note").hidden = !parts.length;
  $("limit-mode-note-text").textContent = parts.join("；") + "。";
}
function totalRow() {
  const key = chosen(), quota = draft.total_quota, usage = key.total_usage;
  const denied = draft.total_access === "deny";
  const valid = draft.period === key.policy.period && JSON.stringify(quota) === JSON.stringify(key.policy.total_quota) && draft.total_quota_enabled === key.policy.total_quota_enabled
    && (draft.total_access || "allow") === (key.policy.total_access || "allow");
  const money = quota?.unit === "money", unpriced = money && usage?.unpriced > 0;
  const cell = usageCell();
  if (denied) cell.append(el("span", { class: "usage-note danger-text" }, "所有新请求将被拒绝"));
  else if (!valid) cell.append(el("span", { class: "usage-note" }, "保存后计算"));
  else cell.append(...(unpriced ? [el("div", { class: "usage-text" }, "待计价")] : meterBlock(money ? usage?.cost : usage?.tokens, quota, "总额度已耗尽")),
    ...usageExtras(usage, money ? usage?.unpriced : 0, () => reviewModel("")));
  const access = accessSwitch(draft.total_access || "allow", "全部模型权限", (value) => {
    // Allowing all models needs a limit; ask for one before changing the draft.
    if (value === "allow" && !draft.total_quota) { renderDetail(); addRestriction("all", "allow"); return; }
    draft.total_access = value; markDirty(); renderDetail(); refocusAccess("全部模型权限");
  }, !key.active).root;
  const editText = quota ? "编辑额度" : "设置额度";
  return el("tr", { class: "total-row" },
    el("td", { class: "cell-scope" }, el("strong", {}, "全部模型"), el("span", { class: "model-family" }, denied ? "禁止该密钥调用任何模型" : "所有获准模型共享")),
    el("td", { class: "cell-access", "data-label": "权限" }, access),
    quotaCell(denied ? null : quota, denied ? "deny" : "allow"),
    cell,
    actionsCell(
      editButton(editText, editText.slice(0, 2) + "总额度", { "data-quota-total": "true", disabled: !chosen().active, onclick: () => editQuota() }),
      el("button", { class: "ghost icon-button small danger-hover", type: "button", title: "关闭全部模型限制", "aria-label": "关闭全部模型限制", disabled: !chosen().active,
        onclick: () => { draft.total_quota_enabled = false; markDirty(); renderDetail(); } }, icon("power"))));
}
function usageCell() { return el("td", { class: "cell-usage", "data-label": "本周期用量" }); }
// Models a money quota depends on: every model for "all", the channel's models, or one model.
function missingPrices(scope, id) {
  const models = snapshot.models.filter((model) => !model.targets?.length);
  if (scope === "model") return id && !models.find((model) => model.id === id)?.price ? [id] : [];
  return models.filter((model) => (scope === "all" || model.channel === id) && !model.price).map((model) => model.id);
}
// Fill missing prices from public catalogs and wait until the background run finishes.
async function syncMissingPrices() {
  await api("/sync-prices", "POST");
  const deadline = Date.now() + 120000;
  do {
    await new Promise((resolve) => setTimeout(resolve, 1000));
    await load(true);
  } while (snapshot.price_sync?.running && Date.now() < deadline);
}
// Money-quota readiness inside quota dialogs. `read()` returns { scope, id, usage, active }; `refresh` re-renders the dialog.
function priceNotice(read, refresh) {
  const text = el("span");
  let syncing = false;
  const button = el("button", { class: "secondary small", type: "button", onclick: () => guard(async () => {
    syncing = true; render();
    try {
      await syncMissingPrices();
      const left = missingPrices(read().scope, read().id).length;
      notify(left ? "已完成价格同步，仍有 " + left + " 个模型未找到公开价格" : "缺失价格已补全");
    } finally { syncing = false; refresh(); }
  }) });
  const node = el("div", { class: "quota-warning price-notice", hidden: true }, text, button);
  const render = () => {
    const { scope, id, usage, active } = read();
    const missing = active ? missingPrices(scope, id) : [], unpriced = active ? usage?.unpriced || 0 : 0;
    const names = missing.slice(0, 3).join("、") + (missing.length > 3 ? " 等 " + missing.length + " 个模型" : "");
    const parts = [];
    if (missing.length) parts.push(scope === "model" ? "该模型尚未设置价格，无法保存金额额度。"
      : names + " 尚未设置价格，金额额度下" + (missing.length > 1 ? "这些模型" : "该模型") + "的请求会被拒绝。");
    if (unpriced) parts.push("本周期有 " + unpriced + " 条用量尚未计价，保存时需确认按当前单价补计。");
    text.textContent = parts.join("");
    node.hidden = !parts.length;
    const running = syncing || !!snapshot.price_sync?.running;
    button.hidden = !missing.length;
    button.disabled = running;
    button.replaceChildren(icon("refresh"), running ? "正在同步…" : "同步缺失价格");
    button.classList.toggle("is-running", running);
    return { missing, unpriced };
  };
  return { node, render };
}
function quotaCell(quota, access) {
  return el("td", { class: "cell-quota", "data-label": "额度上限" }, quota
    ? [el("strong", { class: "quota-value" }, formatAmount(quota.limit) + " " + unitLabel(quota.unit)), el("span", { class: "model-family" }, PERIOD_LABELS[draft.period])]
    : el("span", { class: "muted" }, access === "deny" ? "禁止调用" : draft.total_quota_enabled ? "仅受总额度限制" : "不限额"));
}
function meterBlock(used, quota, exhaustedText) {
  const unit = unitLabel(quota.unit), meter = quotaMeter(used, quota.limit), exhausted = meter.state === "exhausted";
  return [
    el("div", { class: "usage-head" }, el("span", { class: "usage-text" }, formatAmount(meter.consumed) + " " + unit), meter.percent),
    meter.bar,
    el("div", { class: "usage-note" + (exhausted ? " danger-text" : "") }, exhausted ? exhaustedText : "剩余 " + formatAmount(Math.max(0, meter.max - meter.consumed)) + " " + unit),
  ];
}
function usageExtras(usage, unpriced, review) {
  const meta = [
    unpriced ? el("span", { class: "badge warn" }, unpriced + " 条未计价") : null,
  ].filter(Boolean);
  return [
    meta.length ? el("div", { class: "usage-meta" }, meta) : null,
    usage?.review ? el("button", { class: "review-action small", type: "button", onclick: () => guard(review) }, icon("alert"), "处理待核对（" + usage.review + "）") : null,
  ].filter(Boolean);
}
function actionsCell(...buttons) { return el("td", { class: "cell-actions" }, el("div", { class: "row-actions" }, buttons)); }
function editButton(text, label, attrs) {
  return el("button", { class: "secondary small edit-action", type: "button", title: label, "aria-label": label, ...attrs }, icon("pencil"), el("span", { class: "btn-label" }, text));
}
function removeButton(label, disabled, onclick) {
  return el("button", { class: "ghost icon-button small danger-hover", type: "button", title: "移除限制", "aria-label": "移除 " + label + " 限制", disabled, onclick }, icon("trash"));
}
function ensureRule(model) {
  if (!draft.rules[model]) draft.rules[model] = { access: "allow", quota: null };
  return draft.rules[model];
}
function modelRow(model) {
  const key = chosen(), rule = draft.rules[model];
  const aliases = snapshot.models.filter((m) => m.targets?.length === 1 && m.targets[0] === model).map((m) => m.id);
  const access = accessSwitch(rule.access, model + " 权限", (value) => {
    ensureRule(model).access = value;
    markDirty(); renderDetail(); refocusAccess(model + " 权限");
  }, !key.active).root;
  const usage = key.quotas[model];
  const cell = usageCell();
  if (rule?.quota && usage && draft.rule_mode === key.policy.rule_mode && draft.period === key.policy.period && JSON.stringify(rule.quota) === JSON.stringify(key.policy.rules[model]?.quota)) {
    cell.append(...meterBlock(rule.quota.unit === "tokens" ? usage.tokens : usage.cost, rule.quota, "额度已耗尽"),
      ...usageExtras(usage, usage.unpriced, () => reviewModel(model)));
  } else cell.append(el("span", { class: "usage-note" }, rule?.quota ? "保存后刷新周期用量" : "持续计入密钥总用量"));
  const text = rule?.quota ? "编辑额度" : "设置额度";
  const action = editButton(text, model + " " + text, { "data-quota-model": model, disabled: !key.active, onclick: () => editQuota(model) });
  return el("tr", {},
    el("td", { class: "cell-scope" }, el("span", { class: "model-name" }, model), el("span", { class: "model-family" }, "单个模型 · " + group(model) + (aliases.length ? " · 别名：" + aliases.join(", ") : ""))),
    el("td", { class: "cell-access", "data-label": "权限" }, access), quotaCell(rule?.quota, rule?.access), cell,
    actionsCell(action, removeButton(model, !key.active, () => { delete draft.rules[model]; markDirty(); renderDetail(); })));
}
function channelRow(channel) {
  const key = chosen(), rule = draft.channel_rules[channel.id], usage = key.channel_usage[channel.id];
  const models = snapshot.models.filter((model) => model.channel === channel.id && !model.targets?.length);
  const access = accessSwitch(rule.access, channel.label + " 渠道权限", (value) => {
    draft.channel_rules[channel.id].access = value;
    markDirty(); renderDetail(); refocusAccess(channel.label + " 渠道权限");
  }, !key.active).root;
  const quota = rule?.quota, money = quota?.unit === "money";
  const valid = draft.period === key.policy.period && draft.rule_mode === key.policy.rule_mode &&
    JSON.stringify(rule) === JSON.stringify(key.policy.channel_rules[channel.id]);
  const cell = usageCell();
  if (!valid) cell.append(el("span", { class: "usage-note" }, "保存后刷新周期用量"));
  else {
    if (quota && !(money && usage.unpriced)) cell.append(...meterBlock(money ? usage.cost : usage.tokens, quota, "渠道额度已耗尽"));
    else cell.append(el("div", { class: "usage-text" }, money && usage.unpriced ? "待计价" : formatAmount(money ? usage.cost : usage.tokens) + (money ? " USD" : " Token")));
    cell.append(...usageExtras(usage, 0, () => reviewModel("", channel.id)));
  }
  return el("tr", {},
    el("td", { class: "cell-scope" }, el("strong", {}, channel.label), el("details", { class: "channel-models" },
      el("summary", {}, "渠道 · " + models.length + " 个已知模型"), el("ul", {}, models.length ? models.map((model) => el("li", {}, model.id)) : el("li", {}, "暂未发现模型，可预先配置规则")))),
    el("td", { class: "cell-access", "data-label": "权限" }, access),
    quotaCell(quota, rule?.access),
    cell,
    actionsCell(
      editButton(quota ? "编辑额度" : "设置额度", channel.label + " 渠道" + (quota ? "编辑额度" : "设置额度"),
        { disabled: !key.active, "data-quota-channel": channel.id, onclick: () => editQuota(channel.id, true) }),
      removeButton(channel.label + " 渠道", !key.active, () => { delete draft.channel_rules[channel.id]; markDirty(); renderDetail(); })));
}
function editQuota(model = null, channel = false, enableTotal = false) {
  const total = model === null, key = chosen(), ruleField = channel ? "channel_rules" : "rules", existing = total ? null : draft[ruleField][model];
  const label = total ? "密钥总额度" : channel ? (snapshot.channels.find((item) => item.id === model).label + " 渠道额度") : "单模型额度";
  const quota = total ? draft.total_quota : existing?.quota, usage = total ? key.total_usage : channel ? key.channel_usage[model] : key.quotas[model];
  let unit = quota?.unit || (total ? "tokens" : "none");
  const amounts = { tokens: unit === "tokens" ? quota?.limit || "" : "", money: unit === "money" ? quota?.limit || "" : "" };
  const amount = numericField({ value: quota?.limit || "", "aria-label": "额度上限", placeholder: "输入额度上限" }, unit);
  const amountLabel = el("span");
  const amountField = field("", el("div", {}, amountLabel, amount.input, amount.message));
  amountField.className = "quota-amount-field";
  const help = el("p", { class: "fine" });
  const problem = el("p", { class: "quota-warning", hidden: true });
  const limitWarning = el("p", { class: "quota-warning", hidden: true });
  const blocked = total ? draft.total_access === "deny" : existing?.access === "deny";
  const prices = priceNotice(() => ({ scope: total ? "all" : channel ? "channel" : "model", id: model, usage, active: unit === "money" && !blocked }), () => update());
  const updateLimitWarning = () => {
    const active = !blocked && (total ? enableTotal || draft.total_quota_enabled : true);
    const used = unit === "money" ? usage?.cost : usage?.tokens;
    const known = usage && draft.period === key.policy.period && !(unit === "money" && usage.unpriced);
    const value = amount.input.value.trim();
    limitWarning.hidden = !active || unit === "none" || !known || value === "" || !Number.isFinite(Number(value)) || Number(value) < 0 || Number(value) > Number(used);
    limitWarning.textContent = "本周期已记录用量达到或超过此上限，保存后将立即拦截该额度范围的新请求。";
  };
  amount.input.addEventListener("input", updateLimitWarning);
  const usedText = el("strong");
  const context = el("div", { class: "quota-context" },
    el("div", {}, el("span", { class: "fine" }, "额度周期"), el("strong", {}, ({ day: "每天", week: "每周", month: "每月" })[draft.period]), el("small", { class: "muted" }, "沿用当前密钥设置")),
    el("div", {}, el("span", { class: "fine" }, "本周期已用"), usedText,
      el("small", { class: "muted" }, draft.period === key.policy.period ? date(key.period_reset_at) + " 重置" : "周期修改将在保存后生效")));
  const modes = el("div", { class: "quota-modes", role: "radiogroup", "aria-label": "额度类型" });
  const update = () => {
    [...modes.querySelectorAll("input")].forEach((node) => { node.checked = node.value === unit; node.parentElement.classList.toggle("active", node.checked); });
    amountField.hidden = unit === "none"; amount.input.disabled = unit === "none"; amount.input.required = unit !== "none";
    amount.input.dataset.unit = unit; amount.input.inputMode = unit === "tokens" ? "numeric" : "decimal";
    amount.input.removeAttribute("aria-invalid"); amount.message.hidden = true;
    amountLabel.textContent = unit === "tokens" ? "Token 上限" : "金额上限（USD）";
    help.textContent = total ? "全部模型共享。启用后，0 表示阻止新请求；关闭总额度会保留配置与用量。" :
      unit === "none" ? "移除此范围的单独额度，保留权限；已启用的密钥总额度仍生效。" :
      channel ? "该渠道全部实际模型共享额度；金额按各模型自己的价格合计。" : "限制该模型用量。0 表示阻止该模型的新请求。";
    // A single-model money quota cannot be saved before that model has a price.
    const { missing } = prices.render();
    const unavailable = !total && !channel && missing.length > 0;
    problem.hidden = !blocked;
    problem.textContent = (total ? "全部模型已禁止使用" : channel ? "该渠道已禁止调用" : "该模型当前禁止调用") + "；保存额度不会开放权限。";
    $("dialog-confirm").disabled = unavailable;
    usedText.textContent = draft.period !== key.policy.period || !usage ? "保存后计算" :
      unit === "money" ? (usage.unpriced ? "待计价" : Number(usage.cost).toLocaleString("zh-CN", { maximumFractionDigits: 6 }) + " USD") : Number(usage.tokens).toLocaleString("zh-CN") + " Token";
    updateLimitWarning();
  };
  for (const [value, label] of (total ? [["tokens", "Token"], ["money", "金额 USD"]] : [["none", "不限额"], ["tokens", "Token"], ["money", "金额 USD"]])) {
    modes.append(el("label", {}, el("input", { type: "radio", name: "quota-unit", value, onchange: () => {
      if (unit !== "none") amounts[unit] = amount.input.value;
      unit = value; amount.input.value = amounts[unit] || ""; update();
    } }), el("span", {}, label)));
  }
  const consent = el("input", { type: "checkbox" });
  const consentField = field("按当前单价补计本周期未计价用量", consent); consentField.className = "reprice-consent"; consentField.hidden = true;
  const contents = [el("p", { class: total ? "" : "model-name" }, total ? "当前密钥 · 全部模型共享" : channel ? label : model), context, el("div", {}, el("p", { class: "field-label" }, "额度类型"), modes), amountField, help, problem, prices.node, limitWarning, consentField];
  if (total) contents.push(el("p", { class: "fine" }, (enableTotal || draft.total_quota_enabled ? "保存后总额度启用。" : "保存后总额度仍关闭。") + "汇总本周期已记录用量；更早未记录的请求不会补算。"));
  modal("配置" + label, contents, async () => {
    const candidate = structuredClone(draft);
    if (total) { candidate.total_quota = { unit, limit: amount.input.value.trim() }; if (enableTotal) candidate.total_quota_enabled = true; }
    else if (unit === "none") { if (candidate[ruleField][model]) candidate[ruleField][model].quota = null; }
    else {
      candidate[ruleField][model] ||= { access: "allow", quota: null };
      candidate[ruleField][model].quota = { unit, limit: amount.input.value.trim() };
    }
    try {
      await api("/policy", "PUT", { key_id: selected, policy: candidate, reprice: consent.checked });
      dirty = false; await load(); notify("额度已保存，对新请求生效");
    } catch (error) {
      if (error.code === "reprice_required") { consentField.hidden = false; consent.required = true; }
      throw error;
    }
  }, enableTotal ? "保存并启用" : "保存额度", "shield");
  update();
}
function addRestriction(initialScope = "channel", presetAccess = null) {
  const key = chosen();
  let scope = initialScope, unit = scope === "all" ? "tokens" : "none";
  const amounts = { tokens: "", money: "" };
  const scopeModes = el("div", { class: "quota-modes scope-modes", role: "radiogroup", "aria-label": "限制范围" });
  const quotaModes = el("div", { class: "quota-modes", role: "radiogroup", "aria-label": "额度类型" });
  const quotaGroup = el("div", {}, el("p", { class: "field-label" }, "额度类型"), quotaModes);
  const firstNewChannel = snapshot.channels.find((item) => !draft.channel_rules[item.id]) || snapshot.channels[0];
  const channelSelect = dropdown(snapshot.channels.map((item) => [item.id, item.label]), firstNewChannel?.id || "other", "选择渠道");
  const modelPicker = modelCombobox(snapshot.models.filter((model) => !model.targets?.length).map((model) => model.id));
  const modelInput = modelPicker.input;
  const accessSelect = accessSwitch("allow", "权限", () => update());
  const channelField = el("div", { class: "field-group" }, el("p", { class: "field-label" }, "选择渠道"), channelSelect.root);
  const modelField = el("div", { class: "field-group" }, el("label", { class: "field-label", for: modelInput.id }, "实际模型"), modelPicker.root);
  const accessField = el("div", { class: "field-group access-field" }, el("p", { class: "field-label" }, "权限"), accessSelect.root);
  const targetRow = el("div", { class: "field-row" }, channelField, modelField, accessField);
  const amount = numericField({ "aria-label": "额度上限", placeholder: "输入额度上限" }, unit);
  const amountLabel = el("span");
  const amountField = field("", el("div", {}, amountLabel, amount.input, amount.message));
  amountField.className = "quota-amount-field";
  const help = el("p", { class: "fine" });
  const modeWarning = el("p", { class: "quota-warning", hidden: true });
  const prices = priceNotice(() => ({
    scope, id: scope === "all" ? "" : target(),
    usage: scope === "all" ? key.total_usage : scope === "channel" ? key.channel_usage?.[target()] : key.quotas?.[target()],
    active: unit === "money" && accessSelect.value === "allow" && (scope !== "model" || !!target()),
  }), () => update());
  const consent = el("input", { type: "checkbox" });
  const consentField = field("按当前单价补计本周期未计价用量", consent);
  consentField.className = "reprice-consent"; consentField.hidden = true;
  const scopeLabels = [
    ...(draft.total_quota_enabled || initialScope === "all" ? [["all", "全部模型"]] : []),
    ["channel", "渠道"],
    ["model", "单个模型"],
  ];
  for (const [value, label] of scopeLabels) {
    scopeModes.append(el("label", {}, el("input", { type: "radio", name: "restriction-scope", value, onchange: () => {
      scope = value;
      loadSelection();
    } }), el("span", {}, label)));
  }
  for (const [value, label] of [["none", "不限额"], ["tokens", "Token"], ["money", "金额 USD"]]) {
    quotaModes.append(el("label", { "data-quota-option": value }, el("input", { type: "radio", name: "restriction-unit", value, onchange: () => {
      if (unit !== "none") amounts[unit] = amount.input.value;
      unit = value; amount.input.value = amounts[unit] || ""; update();
    } }), el("span", {}, label)));
  }
  const target = () => scope === "channel" ? channelSelect.value : modelInput.value.trim();
  const existing = () => scope === "all"
    ? (draft.total_quota || draft.total_access === "deny" ? { access: draft.total_access || "allow", quota: draft.total_quota } : null) :
    draft[scope === "channel" ? "channel_rules" : "rules"][target()];
  const loadSelection = () => {
    const current = existing();
    accessSelect.value = (scope === "all" && presetAccess) || current?.access || "allow";
    presetAccess = null;
    unit = current?.quota?.unit || (scope === "all" ? "tokens" : "none");
    amounts.tokens = current?.quota?.unit === "tokens" ? current.quota.limit : "";
    amounts.money = current?.quota?.unit === "money" ? current.quota.limit : "";
    amount.input.value = unit === "none" ? "" : amounts[unit];
    update();
  };
  const update = () => {
    [...scopeModes.querySelectorAll("input")].forEach((node) => { node.checked = node.value === scope; node.parentElement.classList.toggle("active", node.checked); });
    const denied = accessSelect.value === "deny";
    if (denied && unit !== "none") { amounts[unit] = amount.input.value; unit = "none"; }
    if (!denied && scope === "all" && unit === "none") { unit = existing()?.quota?.unit || "tokens"; amount.input.value = amounts[unit] || ""; }
    quotaModes.querySelector('[data-quota-option="none"]').hidden = scope === "all";
    quotaGroup.hidden = denied;
    amountField.hidden = denied || unit === "none";
    channelField.hidden = scope !== "channel";
    modelField.hidden = scope !== "model";
    if (scope !== "model") modelPicker.close();
    accessField.hidden = false;
    targetRow.hidden = false;
    modelInput.required = scope === "model";
    [...quotaModes.querySelectorAll("input")].forEach((node) => { node.checked = node.value === unit; node.parentElement.classList.toggle("active", node.checked); });
    amount.input.disabled = denied || unit === "none";
    amount.input.required = !denied && unit !== "none";
    amount.input.dataset.unit = unit;
    amount.input.inputMode = unit === "tokens" ? "numeric" : "decimal";
    amountLabel.textContent = unit === "tokens" ? "Token 上限" : "金额上限（USD）";
    const prefix = existing() ? "该范围已有设置，本次保存会更新它。 " : "";
    help.textContent = prefix + (scope === "all" ? (denied ? "保存后该密钥的所有新请求都会被拒绝；已设置的总额度会保留。" : "全部获准模型共享此额度。") :
      denied ? "禁止规则无需额度；保存后该范围的新请求将被拒绝。" :
      unit === "none" ? "只添加允许权限，不设置独立额度。" :
      scope === "channel" ? "渠道内所有实际模型共享额度；金额按各模型价格汇总。" : "只限制这个实际模型。");
    const targetMode = scope === "channel" ? "channel" : scope === "model" ? "model" : draft.rule_mode;
    const switching = scope !== "all" && targetMode !== draft.rule_mode && Object.keys(activeRules(draft)).length > 0;
    modeWarning.hidden = !switching;
    modeWarning.textContent = switching
      ? "保存后将切换为按" + modeLabel(targetMode) + "限制；当前 " + Object.keys(activeRules(draft)).length + " 条" + modeLabel(draft.rule_mode) + "限制会保留但暂停。" : "";
    const { missing } = prices.render();
    const missingPrice = scope === "model" && missing.length > 0;
    $("dialog-confirm").disabled = missingPrice;
  };
  channelSelect.onchange = loadSelection; modelInput.oninput = loadSelection;
  amount.input.addEventListener("input", () => { if (unit !== "none") amounts[unit] = amount.input.value; });
  modal("添加限制", [
    el("div", {}, el("p", { class: "field-label" }, "限制范围"), scopeModes),
    targetRow,
    quotaGroup,
    amountField, help, modeWarning, prices.node, consentField,
  ], async () => {
    const candidate = structuredClone(draft);
    if (scope === "all") {
      candidate.total_quota_enabled = true;
      candidate.total_access = accessSelect.value;
      if (accessSelect.value === "allow") candidate.total_quota = { unit, limit: amount.input.value.trim() };
    } else {
      const id = target();
      if (!id) throw new Error(scope === "channel" ? "请选择渠道" : "请输入实际模型 ID");
      candidate.rule_mode = scope;
      const fieldName = scope === "channel" ? "channel_rules" : "rules";
      candidate[fieldName][id] = {
        access: accessSelect.value,
        quota: unit === "none" ? null : { unit, limit: amount.input.value.trim() },
      };
    }
    try {
      await api("/policy", "PUT", { key_id: selected, policy: candidate, reprice: consent.checked });
      dirty = false; await load(); notify("限制已添加，对新请求生效");
    } catch (error) {
      if (error.code === "reprice_required") { consentField.hidden = false; consent.required = true; }
      throw error;
    }
  }, "添加限制", "plus");
  loadSelection();
}
async function savePolicy(reprice = false) {
  await api("/policy", "PUT", { key_id: selected, policy: draft, reprice });
  dirty = false; await load(); notify("配置已保存，对新请求生效");
}
$("save-policy").onclick = async () => {
  if (savingPolicy) return;
  const invalidModel = Object.entries(draft.rules).find(([, rule]) => rule.quota && !(rule.quota.unit === "tokens" ? /^\d+$/ : /^\d+(?:\.\d{0,6})?$/).test(rule.quota.limit.trim()));
  if (invalidModel) { notify("模型「" + invalidModel[0] + "」的额度格式无效，请重新编辑。", true); return; }
  savingPolicy = true; const restore = pendingButton($("save-policy"), "保存中…");
  try { await savePolicy(); }
  catch (error) {
    if (error.code === "reprice_required") modal("确认补计历史费用", [el("p", {}, error.message)], () => savePolicy(true), "补计并保存", "alert");
    else notify(error.message, true);
  }
  finally { savingPolicy = false; restore(); $("save-policy").disabled = !dirty || !chosen()?.active; }
};
$("key-note").oninput = () => { draft.note = $("key-note").value; markDirty(); };
$("total-quota-enabled").onchange = () => {
  if ($("total-quota-enabled").checked && !draft.total_quota && draft.total_access !== "deny") {
    $("total-quota-enabled").checked = false; addRestriction("all"); return;
  }
  draft.total_quota_enabled = $("total-quota-enabled").checked; markDirty(); renderDetail();
};
const periodDropdown = dropdown([["day", "每天"], ["week", "每周"], ["month", "每月"]], "day", "额度周期");
periodDropdown.button.id = "key-period";
$("key-period-slot").replaceWith(periodDropdown.root);
periodDropdown.onchange = (value) => { draft.period = value; markDirty(); renderDetail(); };
$("discard").onclick = () => { draft = structuredClone(chosen().policy); dirty = false; renderDetail(); };
$("key-search").oninput = () => { $("clear-key-search").hidden = !$("key-search").value; scheduleKeySearch(); };
$("key-search").onkeydown = (event) => {
  if (event.key === "Escape" && $("key-search").value) { event.preventDefault(); clearKeySearch(); }
  else if (event.key === "ArrowDown") { event.preventDefault(); $("key-list").querySelector(".key-card-select")?.focus(); }
};
$("clear-key-search").onclick = () => { clearKeySearch(); $("key-search").focus(); };
// Roving focus between key cards; Enter/Space select through the native button.
$("key-list").addEventListener("keydown", (event) => {
  const moves = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
  if (!(event.key in moves) && event.key !== "Home" && event.key !== "End") return;
  const buttons = [...$("key-list").querySelectorAll(".key-card-select")];
  const index = buttons.indexOf(document.activeElement);
  if (index < 0) return;
  event.preventDefault();
  if (event.key === "ArrowUp" && index === 0) { $("key-search").focus(); return; }
  const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : Math.min(buttons.length - 1, Math.max(0, index + moves[event.key]));
  buttons[next].focus();
  revealCard(buttons[next].closest(".key-card"));
});
$("reveal-all-keys").onclick = () => guard(async () => {
  const activeKeys = snapshot?.keys.filter((key) => key.active) || [];
  if (activeKeys.length === 0) return;
  if (revealAll) {
    revealAll = false;
    revealedKeys.clear();
    renderKeys();
    notify("完整密钥已隐藏");
    return;
  }
  await revealMissingKeys(true);
  notify("已显示全部有效密钥");
});
function pruneRevealedKeys() {
  for (const keyId of revealedKeys.keys()) if (!snapshot?.keys.some((key) => key.active && key.id === keyId)) revealedKeys.delete(keyId);
}
// Read plain keys for every active key not yet revealed (new keys appearing while the mode is on).
async function revealMissingKeys(enable = false) {
  if (!enable && !revealAll) return;
  const missing = (snapshot?.keys || []).filter((key) => key.active && !revealedKeys.has(key.id));
  if (enable) revealAll = true;
  if (!missing.length || revealingKeys) { renderKeys(); return; }
  const currentToken = token;
  revealingKeys = true; renderRevealAllKeys();
  try {
    const values = await Promise.all(missing.map(async (key) => [key.id, await rawKey(key.id)]));
    if (token !== currentToken || !revealAll) return;
    for (const [keyId, value] of values) revealedKeys.set(keyId, value);
  } catch (error) {
    if (enable) { revealAll = false; throw error; }
    notify(error.message, true);
  } finally {
    revealingKeys = false;
    if (snapshot) renderKeys(); else renderRevealAllKeys();
  }
}
$("add-restriction").onclick = () => addRestriction();
$("empty-add-restriction").onclick = () => addRestriction();
$("sync-models").onclick = () => guard(async () => {
  const restore = pendingButton($("sync-models"), "同步中…");
  try { const result = await api("/sync-models", "POST", { key_id: selected }); await load(true); notify("已同步 " + result.count + " 个模型"); }
  finally { restore(); $("sync-models").disabled = !chosen()?.active; }
});
const PRICE_FILTERS = {
  all: () => true,
  missing: (model) => !model.price,
  automatic: (model) => !!model.price && model.price_source !== "manual",
  manual: (model) => model.price_source === "manual",
};
// Fixed-point price strings keep six decimals; trim trailing zeros for scanning but keep two.
function priceText(value) {
  if (value === null || value === undefined || value === "") return null;
  const [whole, fraction = ""] = String(value).split(".");
  return whole + "." + fraction.replace(/0+$/, "").padEnd(2, "0");
}
function renderPrices(preserveFocus = false) {
  const models = snapshot.models.filter((m) => !m.targets?.length);
  const search = $("price-search").value.trim().toLowerCase(), filter = priceFilter;
  const matching = models.filter((model) => model.id.toLowerCase().includes(search));
  const visible = matching.filter(PRICE_FILTERS[filter]);
  for (const button of $("price-filter").querySelectorAll("[data-filter]")) {
    button.setAttribute("aria-pressed", String(button.dataset.filter === filter));
    button.querySelector(".segment-count").textContent = matching.filter(PRICE_FILTERS[button.dataset.filter]).length;
  }
  $("clear-price-search").hidden = !$("price-search").value;
  $("price-count").textContent = "显示 " + visible.length + " / " + models.length + " 个模型";
  const sync = snapshot.price_sync || {}, priced = models.filter((model) => model.price).length;
  $("price-coverage").textContent = priced + " / " + models.length + " 已设置";
  $("price-coverage").className = "badge" + (priced < models.length ? " warn" : "");
  $("sync-prices").disabled = !!sync.running;
  $("sync-prices").replaceChildren(icon("refresh"), sync.running ? "正在补全…" : "同步缺失价格");
  $("sync-prices").classList.toggle("is-running", !!sync.running);
  document.querySelector(".price-sync-bar").classList.toggle("is-running", !!sync.running);
  $("price-sync-status").textContent = sync.running ? "正在后台补全缺失价格" : (sync.enabled ? "自动补全已开启 · 每 " + sync.interval_hours + " 小时检查" : "自动补全已关闭，可手动同步");
  const details = [];
  if (sync.last_success) details.push("上次完成 " + date(sync.last_success) + "，新增 " + sync.added + " 个价格");
  if (sync.error) details.push(sync.error);
  else if (sync.unmatched?.length) details.push(sync.unmatched.length + " 个模型未找到唯一匹配，可手动设置");
  const errors = (sync.sources || []).filter((source) => source.error).map((source) => source.source + "：" + source.error);
  if (errors.length) details.push(errors.join("；"));
  $("price-sync-detail").textContent = details.join("。") || "首次同步会在后台完成，已设置的价格不会被覆盖。";
  $("price-sync-detail").classList.toggle("error", !!sync.error);
  const signature = JSON.stringify([search, filter, visible]);
  if (signature === priceRowsSignature || (preserveFocus === true && document.activeElement?.closest("#price-rows"))) return schedulePriceRefresh();
  priceRowsSignature = signature;
  $("price-rows").replaceChildren(...visible.map((model) => el("tr", { class: model.price ? "" : "missing-row" },
    el("td", { class: "price-name" }, el("strong", { title: model.id }, model.id)),
    el("td", {}, el("span", { class: "channel-tag" }, group(model.id))),
    ...["input", "output", "cache_read", "cache_write"].map((name) => {
      const value = model.price ? priceText(model.price[name]) : null;
      return el("td", { class: "price-value num" + (value ? "" : " muted"), title: model.price?.[name] ?? "" }, value ?? (model.price ? "沿用输入" : "—"));
    }),
    el("td", {}, el("span", { class: "price-source" },
      el("span", { class: "badge" + (!model.price ? " warn" : model.price_source === "manual" ? " info" : "") }, model.price_source === "manual" ? "手动设置" : (model.price_source || "待设置")),
      model.price_synced_at ? el("small", { class: "price-origin", title: model.price_source_model || "" }, date(model.price_synced_at, false)) : null)),
    el("td", { class: "cell-actions" }, el("div", { class: "row-actions" },
      el("button", { class: (model.price ? "secondary" : "primary") + " small", type: "button", "data-model": model.id, onclick: () => editPrice(model) },
        icon(model.price ? "pencil" : "plus"), model.price ? "编辑" : "设置价格"))))));
  if (!visible.length) $("price-rows").append(el("tr", {}, el("td", { colSpan: 8, class: "empty compact" },
    icon("models"), el("h2", {}, models.length ? "没有匹配的价格" : "暂无模型"),
    el("p", {}, models.length ? "调整模型名称或价格状态后重试。" : "请先在 API 密钥页面同步或添加模型。"),
    models.length ? el("button", { class: "secondary", onclick: () => { $("price-search").value = ""; priceFilter = "all"; renderPrices(); } }, "清除筛选") : null)));
  schedulePriceRefresh();
}
$("price-search").oninput = renderPrices;
$("price-search").onkeydown = (event) => {
  if (event.key === "Escape" && $("price-search").value) { event.preventDefault(); $("price-search").value = ""; renderPrices(); }
};
$("clear-price-search").onclick = () => { $("price-search").value = ""; renderPrices(); $("price-search").focus(); };
$("price-filter").onclick = (event) => {
  const button = event.target.closest("[data-filter]");
  if (!button || button.dataset.filter === priceFilter) return;
  priceFilter = button.dataset.filter; renderPrices();
};
function schedulePriceRefresh() {
  clearTimeout(priceTimer);
  if (!token || !(view === "prices" || snapshot?.price_sync?.running)) return;
  const currentToken = token;
  priceTimer = setTimeout(async () => {
    try {
      const data = await api("/state");
      if (token !== currentToken) return;
      snapshot = data; renderPrices(true);
      pruneRevealedKeys();
      renderCounts();
      if (view === "keys" && !dirty && !$("dialog").open) { renderKeys(); renderDetail(); }
      revealMissingKeys();
    } catch { if (token === currentToken) priceTimer = setTimeout(schedulePriceRefresh, 10000); }
  }, snapshot.price_sync?.running ? 1000 : 30000);
}
$("sync-prices").onclick = () => guard(async () => {
  $("sync-prices").disabled = true;
  try { await api("/sync-prices", "POST"); await load(true); notify("已开始补全缺失价格"); }
  finally { $("sync-prices").disabled = !!snapshot?.price_sync?.running; }
});
function editPrice(model) {
  model = snapshot.models.find((item) => item.id === model.id) || model;
  const fields = {};
  const grid = el("div", { class: "field-grid" });
  for (const [name, label] of [["input", "输入"], ["output", "输出"], ["cache_read", "缓存读取"], ["cache_write", "缓存写入"]]) {
    const required = ["input", "output"].includes(name);
    const control = numericField({ inputMode: "decimal", value: model.price?.[name] || "", required, "aria-label": label + "价（USD / 百万 Token）", placeholder: required ? "必填，例如 1.25" : "留空沿用输入价" }, "money", !required);
    fields[name] = control.input;
    grid.append(field(label + "价", el("div", {}, control.input, control.message)));
  }
  const source = model.price_source === "manual" ? "当前为手动价格" : model.price_source ? "当前来自 " + model.price_source : "尚未设置价格";
  modal("设置模型价格", [
    el("div", { class: "dialog-subject" }, el("span", { class: "model-name" }, model.id), el("span", { class: "outline-tag" }, "USD / 百万 Token")),
    grid,
    el("p", { class: "fine" }, source + "。保存为手动价格后，自动同步不会覆盖；新价格仅影响之后放行的请求。"),
  ], async () => {
    const price = Object.fromEntries(Object.entries(fields).map(([name, node]) => [name, node.value.trim() || null]));
    await api("/price", "PUT", { model: model.id, price, revision: model.revision }); await load(true); notify("模型价格已保存");
  }, "保存价格", "coins");
}
async function reviewModel(model, channel = "") {
  const result = await api("/reviews?" + new URLSearchParams({ key: selected, model, channel }));
  if (!result.items.length) { await load(true); notify(model ? "该模型已无待核对请求" : "当前密钥已无待核对请求"); return; }
  settle(result.items[0], result.items.length);
}
function settle(item, count) {
  const fields = {}, grid = el("div", { class: "field-grid" });
  for (const [name, label] of [["input", "普通输入 Token"], ["output", "输出 Token"], ["cache_read", "缓存读取 Token"], ["cache_write", "缓存写入 Token"]]) {
    fields[name] = el("input", { type: "number", min: "0", step: "1", max: "9000000000000000", required: true, value: "0" }); grid.append(field(label, fields[name]));
  }
  const reason = el("textarea", { required: true, minLength: 4, maxLength: 500, rows: 3, placeholder: "填写数据来源和核对依据" });
  modal("处理待核对", [el("p", {}, item.model + " · 尚有 " + count + " 个请求待处理"), el("p", { class: "fine" }, "当前请求：" + item.request_id), grid, field("核对原因", reason), el("p", { class: "fine" }, "核对后重新校验生效的总额度、渠道或模型额度。之后收到真实用量时，将修正人工记录。")], async () => {
    const tokens = Object.fromEntries(Object.entries(fields).map(([key, node]) => [key, Number(node.value)]));
    if (Object.values(tokens).some((v) => !Number.isSafeInteger(v) || v < 0)) throw new Error("Token 必须为有效的非负整数");
    tokens.total = tokens.input + tokens.output + tokens.cache_read + tokens.cache_write; tokens.complete = true;
    await api("/settle", "POST", { request_id: item.request_id, model: item.model, tokens, reason: reason.value });
    await load(true); notify("已核对该请求");
  }, "确认用量并恢复", "alert");
}
document.querySelectorAll("[data-view]").forEach((button) => button.onclick = () => setView(button.dataset.view));
function openEnforcementSettings() {
  setView("settings"); $("settings-enforcement-enabled").focus({ preventScroll: true });
}
$("plugin-status-chip").onclick = openEnforcementSettings;
$("system-error-action").onclick = openEnforcementSettings;
$("toggle-management-key").onclick = () => setManagementKeyVisible($("settings-management-key").type === "password");
function setManagementKeyVisible(visible) {
  const button = $("toggle-management-key"), label = visible ? "隐藏管理密钥" : "显示管理密钥";
  $("settings-management-key").type = visible ? "text" : "password";
  button.setAttribute("aria-pressed", String(visible)); button.setAttribute("aria-label", label); button.title = label;
  button.replaceChildren(icon(visible ? "eyeOff" : "eye"));
}
$("credential-form").onsubmit = async (event) => {
  event.preventDefault(); $("credential-error").textContent = "";
  const managementKey = $("settings-management-key").value.trim();
  if (!managementKey) { $("credential-error").textContent = "请输入管理密钥"; return; }
  const remember = $("remember-management-key").checked;
  const restore = pendingButton($("save-credential"), "正在连接…");
  token = managementKey; credentialSource = remember ? "saved" : "temporary";
  revealAll = false; revealedKeys.clear(); snapshot = null;
  try {
    await load();
    let storageMessage = "";
    try {
      if (remember) credentials.saveManagementKey(localStorage, managementKey, credentialHost, credentialAgent);
      else credentials.clearManagementKey(localStorage);
    } catch {
      credentialSource = "temporary";
      storageMessage = "已连接，但浏览器未允许保存管理密钥，本次仅在当前页面有效。";
    }
    $("settings-management-key").value = "";
    setManagementKeyVisible(false);
    renderCredentialState(storageMessage);
    setView("keys");
    notify(storageMessage || "管理密钥已验证并连接");
  } catch (error) {
    if (error.status !== 401) renderCredentialState(error.message);
  } finally { restore(); }
};
$("clear-credential").onclick = () => {
  try { credentials?.clearManagementKey(localStorage); }
  catch { $("credential-error").textContent = "浏览器未允许清除本地凭据"; return; }
  if (credentialSource === "saved") disconnectCredential("本页保存的管理密钥已清除。");
  else renderCredentialState();
  notify("已清除本页保存的管理密钥");
};
$("settings-enforcement-enabled").onchange = () => guard(async () => {
  const control = $("settings-enforcement-enabled"), enabled = control.checked;
  savingEnforcement = true; control.disabled = true;
  try {
    const result = await api("/settings", "PUT", { enforcement_enabled: enabled });
    snapshot.enforcement_enabled = result.enforcement_enabled === true;
    renderEnforcementState();
    notify(snapshot.enforcement_enabled ? "请求拦截与记账已启用" : "请求拦截与记账已关闭");
  } catch (error) {
    control.checked = !enabled;
    throw error;
  } finally {
    savingEnforcement = false;
    renderEnforcementState();
  }
});
$("refresh").onclick = () => guard(async () => {
  if (!token) { setView("settings"); return; }
  const restore = pendingButton($("refresh"), "刷新中…");
  try { await load(); notify("数据已刷新"); } finally { restore(); }
});
$("settings-timezone-browser").onclick = () => {
  const browser = browserTimeZone();
  if (browser) guard(() => saveTimeZone(browser));
};
document.addEventListener("keydown", (event) => {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.key.toLowerCase() !== "s") return;
  if (view !== "keys" || $("dialog").open || $("key-detail").hidden) return;
  event.preventDefault();
  if (!$("save-policy").disabled) $("save-policy").click();
});

async function bootstrap() {
  const candidates = [];
  try {
    const hostKey = credentials?.readHostManagementKey(localStorage, credentialHost, credentialAgent) || "";
    const savedKey = savedManagementKey();
    if (hostKey) candidates.push(["host", hostKey]);
    if (savedKey && savedKey !== hostKey) candidates.push(["saved", savedKey]);
  } catch {
    // Browsers may block storage for cross-origin iframes. The settings page remains available.
  }
  let connected = false, message = "";
  for (const [source, managementKey] of candidates) {
    token = managementKey; credentialSource = source;
    try {
      await load();
      connected = true;
      break;
    } catch (error) {
      message = error.message;
      if (error.status !== 401) break;
    }
  }
  $("boot").hidden = true; $("app").hidden = false;
  if (connected) {
    renderCredentialState();
    setView("keys");
  } else {
    if (!token) credentialSource = "";
    setView("settings");
    renderCredentialState(message);
  }
}
bootstrap();
