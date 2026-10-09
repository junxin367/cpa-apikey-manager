"use strict";
const API = "/v0/management/plugins/cpa-apikey-manager";
const $ = (id) => document.getElementById(id);
let token = "", snapshot = null, selected = "", draft = null, dirty = false;
let view = "keys", toastTimer, priceTimer, dialogAction, busy = false;
let dialogDirty = false, dialogTrigger = null, fieldSequence = 0, savingPolicy = false;
let priceRowsSignature = "";
let credentialSource = "";
const credentials = globalThis.CpaCredentials;
const credentialHost = window.location.host, credentialAgent = navigator.userAgent;

function icon(name) {
  const paths = {
    shield: ["M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6l-8-3Z", "m8 12 3 3 5-6"],
    key: ["M14 7a4 4 0 1 1-2 7L6 20H3v-3l6-6a4 4 0 0 1 5-4Z", "M16 8h.01"],
    coins: ["M17 7c0 2-3 3-7 3S3 9 3 7s3-3 7-3 7 1 7 3Z", "M3 7v5c0 2 3 3 7 3", "M3 12v5c0 2 3 3 7 3", "M17 10c3 0 4 1 4 3s-2 3-5 3-5-1-5-3 2-3 5-3", "M11 13v5c0 2 2 3 5 3s5-1 5-3v-5"],
    models: ["M3 3h7v7H3Z", "M14 3h7v7h-7Z", "M3 14h7v7H3Z", "M14 14h7v7h-7Z"],
    clock: ["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z", "M12 7v5l3 2"],
    refresh: ["M20 7v5h-5", "M4 17v-5h5", "M6 7a7 7 0 0 1 12-1l2 6", "M18 17a7 7 0 0 1-12 1l-2-6"],
    settings: ["M4 21v-7", "M4 10V3", "M12 21v-9", "M12 8V3", "M20 21v-5", "M20 12V3", "M1 14h6", "M9 8h6", "M17 16h6"],
    plus: ["M12 5v14", "M5 12h14"],
    close: ["m6 6 12 12", "M18 6 6 18"],
    info: ["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z", "M12 11v6", "M12 7h.01"],
    check: ["m5 12 4 4L19 6"],
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
  button.disabled = true; button.setAttribute("aria-busy", "true"); button.replaceChildren(icon("refresh"), message);
  return () => { button.replaceChildren(...children); button.style.minWidth = width; button.disabled = disabled; button.removeAttribute("aria-busy"); };
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
function select(options, value, change) {
  const node = el("select", { onchange: change });
  for (const [v, label] of options) node.append(el("option", { value: v, text: label }));
  node.value = value;
  return node;
}
function notify(message, error = false) {
  clearTimeout(toastTimer); $("toast").replaceChildren(icon(error ? "info" : "check"), el("span", {}, message));
  ($("dialog").open ? $("dialog") : document.body).append($("toast"));
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
function savedManagementKey() {
  return credentials?.readSavedManagementKey(localStorage, credentialHost, credentialAgent) || "";
}
function renderCredentialState(message = "") {
  const connected = !!token;
  const labels = {
    host: "已自动复用宿主管理中心保存的管理密钥。",
    saved: "已使用本页保存在当前浏览器中的管理密钥。",
    temporary: "已使用当前页面的临时管理密钥，刷新后需要重新设置。",
  };
  $("credential-status").textContent = connected ? "已连接" : "需要管理密钥";
  $("credential-status").className = "badge" + (connected ? "" : " warn");
  $("credential-source").textContent = connected ? labels[credentialSource] : "未找到可用凭据，请在下方保存管理密钥。";
  $("credential-error").textContent = message;
  $("clear-credential").disabled = !savedManagementKey();
  $("refresh").disabled = !connected;
  $("timezone-chip").hidden = !snapshot;
}
function setView(next) {
  if (next !== "settings" && !token) next = "settings";
  view = next;
  const viewName = ({ keys: "API 密钥", prices: "模型价格", settings: "设置" })[view];
  if (snapshot && chosen()) draft = structuredClone(chosen().policy);
  document.querySelectorAll("[data-view]").forEach((node) => {
    node.classList.toggle("active", node.dataset.view === view);
    if (node.dataset.view === view) node.setAttribute("aria-current", "page"); else node.removeAttribute("aria-current");
  });
  for (const name of ["keys", "prices", "settings"]) $(name + "-view").hidden = name !== view;
  $("breadcrumb").textContent = viewName;
  document.title = `${viewName} · CPA 密钥权限与额度`;
  schedulePriceRefresh();
  if (view === "keys" && snapshot) renderDetail();
  if (view === "settings") renderCredentialState();
}
function disconnectCredential(message = "") {
  token = ""; credentialSource = ""; snapshot = null; selected = ""; draft = null; dirty = false;
  clearTimeout(priceTimer);
  $("timezone-chip").hidden = true;
  $("system-error").hidden = true;
  $("plugin-status").textContent = "状态待连接";
  $("plugin-status-dot").classList.add("disabled");
  setView("settings");
  renderCredentialState(message);
}
function modal(title, contents, action, button = "确认") {
  $("dialog-title").textContent = title; $("dialog-body").replaceChildren(...contents);
  $("dialog-error").textContent = ""; $("dialog-confirm").textContent = button;
  $("dialog-confirm").disabled = false; dialogAction = action; dialogDirty = false;
  dialogTrigger = document.activeElement;
  $("dialog-discard").hidden = true; $("dialog-actions").hidden = false;
  document.body.classList.add("modal-open"); $("dialog").showModal();
}
function closeDialog() {
  if (busy) return;
  if (dialogDirty) {
    $("dialog-discard").hidden = false; $("dialog-actions").hidden = true; $("dialog-continue").focus();
  } else $("dialog").close();
}
$("dialog-close").onclick = closeDialog; $("dialog-cancel").onclick = closeDialog;
$("dialog-body").addEventListener("input", () => { dialogDirty = true; });
$("dialog").addEventListener("cancel", (event) => { event.preventDefault(); closeDialog(); });
$("dialog-continue").onclick = () => { $("dialog-discard").hidden = true; $("dialog-actions").hidden = false; $("dialog-body").querySelector("input,textarea")?.focus(); };
$("dialog-abandon").onclick = () => { dialogDirty = false; $("dialog").close(); };
$("dialog").addEventListener("close", () => {
  dialogDirty = false; document.body.classList.remove("modal-open"); document.body.append($("toast"));
  if (dialogTrigger?.isConnected) dialogTrigger.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.model) [...$("price-rows").querySelectorAll("button")].find((node) => node.dataset.model === dialogTrigger.dataset.model)?.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.quotaModel) [...$("limit-rows").querySelectorAll("button")].find((node) => node.dataset.quotaModel === dialogTrigger.dataset.quotaModel)?.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.quotaChannel) [...$("limit-rows").querySelectorAll("button")].find((node) => node.dataset.quotaChannel === dialogTrigger.dataset.quotaChannel)?.focus({ preventScroll: true });
  else if (dialogTrigger?.dataset.quotaTotal) $("limit-rows").querySelector("[data-quota-total]")?.focus({ preventScroll: true });
});
$("dialog-form").onsubmit = async (event) => {
  event.preventDefault(); if (busy || !$("dialog-discard").hidden) return;
  if (![...$("dialog-body").querySelectorAll("input[data-unit]")].filter((input) => !input.disabled).map(validateNumber).every(Boolean)) return;
  busy = true; const restore = pendingButton($("dialog-confirm"), "处理中…");
  try { await dialogAction(); dialogDirty = false; $("dialog").close(); }
  catch (error) { $("dialog-error").textContent = error.message; }
  finally { busy = false; restore(); }
};
function confirmLeave(action) {
  if (!dirty) return guard(action);
  modal("有尚未保存的配置", [el("p", {}, "离开会放弃当前密钥的修改。")], async () => {
    dirty = false; await action();
  }, "放弃修改并继续");
}
function field(label, input) { return el("label", {}, label, input); }
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
function date(value, time = true) {
  const options = { year: "numeric", month: "2-digit", day: "2-digit", ...(time ? { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false } : {}) };
  try { return new Intl.DateTimeFormat("zh-CN", { ...options, timeZone: snapshot.timezone }).format(new Date(value)); }
  catch { return new Intl.DateTimeFormat("zh-CN", { ...options, timeZone: "UTC" }).format(new Date(new Date(value).getTime() + (snapshot.server_offset_seconds || 0) * 1000)); }
}
function group(id) {
  return snapshot?.models.find((model) => model.id === id)?.channel_label || "其他";
}
function activeRules(policy) { return policy.rule_mode === "channel" ? policy.channel_rules : policy.rules; }
function modeLabel(mode) { return mode === "channel" ? "渠道" : "模型"; }
function chosen() { return snapshot?.keys.find((k) => k.id === selected); }
function markDirty() {
  dirty = true; $("dirty-state").textContent = "有未保存的更改";
  $("save-policy").disabled = savingPolicy || !chosen()?.active; $("discard").disabled = false;
}
async function load(preserve = false) {
  const data = await api("/state");
  snapshot = data;
  if (!data.keys.some((key) => key.id === selected)) selected = data.keys.find((k) => k.active)?.id || data.keys[0]?.id || "";
  if (!preserve || !draft) { draft = chosen() ? structuredClone(chosen().policy) : null; dirty = false; }
  render();
}
function render() {
  $("version").textContent = "v" + snapshot.version; $("timezone").textContent = "服务器时区：" + snapshot.timezone;
  $("timezone-chip").hidden = false;
  const enforcementEnabled = snapshot.enforcement_enabled !== false;
  $("plugin-status").textContent = enforcementEnabled ? "拦截已启用" : "拦截未启用";
  $("plugin-status-dot").classList.toggle("disabled", !enforcementEnabled);
  $("key-count").textContent = snapshot.keys.filter((k) => k.active).length;
  $("stat-keys").textContent = $("key-count").textContent;
  $("stat-models").textContent = snapshot.models.filter((model) => !model.targets?.length).length;
  $("stat-rules").textContent = snapshot.keys.reduce((count, key) => count + Object.keys(activeRules(key.policy)).length + (key.policy.total_quota_enabled ? 1 : 0), 0);
  const problem = snapshot.source_error || snapshot.health_error;
  const message = !enforcementEnabled
    ? "权限与额度拦截尚未启用。请在插件配置中开启 enforcement-enabled；开启前，当前请求会直接放行且不记账。"
    : problem ? "当前请求受到保护性拦截：" + problem : "";
  $("system-error").className = "banner " + (!enforcementEnabled ? "warning" : "error");
  $("system-error").textContent = message;
  $("system-error").hidden = !message;
  $("keys-unavailable").hidden = !snapshot.source_error;
  $("keys-content").hidden = !!snapshot.source_error;
  $("keys-unavailable-reason").textContent = snapshot.source_error || "";
  renderKeys(); renderDetail(); renderPrices();
}
function renderKeys() {
  const search = $("key-search").value.trim().toLowerCase();
  const keys = snapshot.keys.filter((k) => (k.masked + k.policy.note).toLowerCase().includes(search));
  $("key-list").replaceChildren(...keys.map((key) => el("button", {
    class: "key-card" + (key.id === selected ? " selected" : ""),
    "aria-pressed": key.id === selected,
    onclick: () => {
      if (key.id === selected) return;
      confirmLeave(() => { selected = key.id; draft = structuredClone(key.policy); dirty = false; renderKeys(); renderDetail(); });
    },
  }, el("strong", {}, icon("key"), key.policy.note || key.masked),
  el("small", { class: "key-mask" }, key.policy.note ? key.masked : "未设置备注"),
  el("span", { class: "key-meta" }, el("small", {}, (key.policy.total_quota_enabled ? "总额度 · " : "") + "按" + modeLabel(key.policy.rule_mode) + " · " + Object.keys(activeRules(key.policy)).length + " 条规则"), el("span", { class: "badge" + (key.active ? "" : " danger") }, key.active ? "有效" : "已删除")))));
  if (!keys.length) $("key-list").append(el("div", { class: "empty compact" }, el("p", {}, search ? "没有匹配的密钥" : "宿主暂无 API 密钥，请先在宿主中创建。"), search ? el("button", { class: "secondary", onclick: () => { $("key-search").value = ""; renderKeys(); } }, "清除搜索") : null));
}
function renderDetail() {
  const key = chosen(); $("key-empty").hidden = !!key; $("key-detail").hidden = !key;
  if (!key) return;
  $("selected-key").textContent = key.policy.note || key.masked;
  $("selected-mask").textContent = key.masked;
  $("key-status").textContent = key.active ? "● 来源有效" : "已从宿主删除";
  $("key-status").className = "badge" + (key.active ? "" : " danger");
  $("key-note").value = draft.note; $("key-period").value = draft.period;
  $("key-note").disabled = !key.active; $("key-period").disabled = !key.active;
  $("period-note").textContent = ({ day: "每天 00:00 重置", week: "每周一 00:00 重置", month: "每月 1 日 00:00 重置" })[draft.period] + "，按服务器时区计算。";
  $("save-policy").disabled = savingPolicy || !dirty || !key.active; $("discard").disabled = !dirty;
  $("dirty-state").textContent = dirty ? "有未保存的更改" : "所有更改已保存";
  $("recording-note").textContent = snapshot.enforcement_enabled === false
    ? "拦截功能尚未启用；当前请求不会被插件拦截或记账。开启后从首次放行请求开始持续记账。"
    : "持续记账启用于 " + date(key.recording_since) + "；插件停用期间无法补算。" +
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
  if (draft.total_quota_enabled && draft.total_quota) rows.push(totalRow());
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
  const parts = [];
  if (!draft.total_quota_enabled && draft.total_quota) parts.push("总额度配置已保留，打开开关后才显示并生效");
  if (Object.keys(activeRules(draft)).length) parts.push("当前按" + modeLabel(draft.rule_mode) + "限制");
  if (paused) parts.push("另有 " + paused + " 条" + (draft.rule_mode === "channel" ? "模型" : "渠道") + "限制已保留但暂停");
  if (!parts.length) parts.push("添加渠道或模型限制时会自动采用对应的配置方式");
  $("limit-mode-note").textContent = parts.join("；") + "。";
}
function totalRow() {
  const key = chosen(), quota = draft.total_quota, usage = key.total_usage;
  const valid = draft.period === key.policy.period && JSON.stringify(quota) === JSON.stringify(key.policy.total_quota) && draft.total_quota_enabled === key.policy.total_quota_enabled;
  const money = quota.unit === "money", unit = money ? "USD" : "Token";
  const used = money ? usage?.cost : usage?.tokens;
  const unpriced = money && usage?.unpriced > 0, consumed = Number(used || 0), limit = Number(quota.limit);
  const format = (value) => Number(value).toLocaleString("zh-CN", { maximumFractionDigits: 6 }) + " " + unit;
  const usageCell = el("td", {}, el("div", { class: "usage-text" }, !valid ? "保存后计算" : unpriced ? "待计价" : format(consumed)));
  if (valid) {
    if (!unpriced) {
      const fill = el("span"); fill.style.width = (limit > 0 ? Math.min(100, consumed / limit * 100) : 100) + "%";
      usageCell.append(el("div", { class: "progress" + (consumed >= limit ? " exhausted" : "") }, fill),
        el("div", { class: "usage-note" }, consumed >= limit ? "总额度已耗尽" : "剩余 " + format(Math.max(0, limit - consumed))));
    }
    usageCell.append(el("div", { class: "usage-note" }, date(key.period_reset_at) + " 重置"));
    if (usage?.pending) usageCell.append(el("div", { class: "usage-note" }, usage.pending + " 个请求待结算"));
    if (money && usage?.unpriced) usageCell.append(el("span", { class: "badge warn" }, usage.unpriced + " 条未计价"));
    if (usage?.review) usageCell.append(el("button", { class: "review-action", onclick: () => guard(() => reviewModel("")) }, "处理待核对（" + usage.review + "）"));
  }
  return el("tr", {},
    el("td", {}, el("strong", {}, "全部模型"), el("span", { class: "model-family" }, "所有获准模型共享")),
    el("td", {}, el("span", { class: "badge" }, "允许")),
    el("td", {}, el("strong", { class: "quota-value" }, format(quota.limit)), el("span", { class: "model-family" }, ({ day: "每天", week: "每周", month: "每月" })[draft.period])),
    usageCell,
    el("td", { class: "row-actions" },
      el("button", { class: "secondary", "data-quota-total": "true", onclick: () => editQuota() }, "编辑"),
      el("button", { class: "ghost danger-text", onclick: () => { draft.total_quota_enabled = false; markDirty(); renderDetail(); } }, "关闭")));
}
function ensureRule(model) {
  if (!draft.rules[model]) draft.rules[model] = { access: "allow", quota: null };
  return draft.rules[model];
}
function modelRow(model) {
  const key = chosen(), rule = draft.rules[model];
  const aliases = snapshot.models.filter((m) => m.targets?.length === 1 && m.targets[0] === model).map((m) => m.id);
  const access = select([["allow", "允许"], ["deny", "禁止"]], rule.access, (e) => {
    ensureRule(model).access = e.target.value;
    markDirty(); renderDetail();
  }); access.setAttribute("aria-label", model + " 权限"); access.disabled = !key.active;
  const quotaCell = el("td", {}, rule?.quota
    ? [el("strong", { class: "quota-value" }, Number(rule.quota.limit).toLocaleString("zh-CN", { maximumFractionDigits: 6 })),
      el("span", { class: "model-family" }, (rule.quota.unit === "tokens" ? "Token" : "USD") + " / " + ({ day: "每天", week: "每周", month: "每月" })[draft.period])]
    : el("span", { class: "muted" }, rule?.access === "deny" ? "禁止调用" : draft.total_quota_enabled ? "仅受总额度限制" : "不限额"));
  const usage = key.quotas[model];
  const usageCell = el("td");
  if (rule?.quota && usage && draft.rule_mode === key.policy.rule_mode && draft.period === key.policy.period && JSON.stringify(rule.quota) === JSON.stringify(key.policy.rules[model]?.quota)) {
    const consumed = rule.quota.unit === "tokens" ? usage.tokens : usage.cost;
    const exhausted = Number(consumed) >= Number(rule.quota.limit);
    const percentage = Number(rule.quota.limit) > 0 ? Math.min(100, Number(consumed) / Number(rule.quota.limit) * 100) : 100;
    const fill = el("span"); fill.style.width = percentage + "%";
    usageCell.append(el("div", { class: "usage-text" }, Number(consumed).toLocaleString("zh-CN", { maximumFractionDigits: 6 }) + (rule.quota.unit === "tokens" ? " Token" : " USD")),
      el("div", { class: "progress" + (exhausted ? " exhausted" : "") }, fill),
      el("div", { class: "usage-note" }, exhausted ? "额度已耗尽" : "剩余 " + Math.max(0, Number(rule.quota.limit) - Number(consumed)).toLocaleString("zh-CN", { maximumFractionDigits: 6 })),
      el("div", { class: "usage-note" }, date(usage.reset_at) + " 重置"));
    if (usage.pending) usageCell.append(el("div", { class: "usage-note" }, usage.pending + " 个请求待结算"));
    if (usage.unpriced) usageCell.append(el("span", { class: "badge warn" }, usage.unpriced + " 条未计价"));
    if (usage.review) usageCell.append(el("button", { class: "review-action", onclick: () => guard(() => reviewModel(model)) }, "处理待核对（" + usage.review + "）"));
  } else usageCell.append(el("span", { class: "usage-note" }, rule?.quota ? "保存后刷新周期用量" : "持续计入密钥总用量"));
  const action = el("button", { class: "secondary", "data-quota-model": model, "aria-label": model + (rule?.quota ? " 编辑额度" : " 设置额度"), disabled: !key.active, onclick: () => editQuota(model) }, rule?.quota ? "编辑额度" : "设置额度");
  return el("tr", {}, el("td", {}, el("span", { class: "model-name" }, model), el("span", { class: "model-family" }, "单个模型 · " + group(model) + (aliases.length ? " · 别名：" + aliases.join(", ") : ""))), el("td", {}, access), quotaCell, usageCell,
    el("td", { class: "row-actions" }, action, el("button", { class: "ghost danger-text", disabled: !key.active, onclick: () => { delete draft.rules[model]; markDirty(); renderDetail(); } }, "移除")));
}
function channelRow(channel) {
  const key = chosen(), rule = draft.channel_rules[channel.id], usage = key.channel_usage[channel.id];
  const models = snapshot.models.filter((model) => model.channel === channel.id && !model.targets?.length);
  const access = select([["allow", "允许"], ["deny", "禁止"]], rule.access, (event) => {
    draft.channel_rules[channel.id].access = event.target.value;
    markDirty(); renderDetail();
  });
  access.setAttribute("aria-label", channel.label + " 渠道权限"); access.disabled = !key.active;
  const quota = rule?.quota, money = quota?.unit === "money";
  const used = money ? Number(usage.cost) : Number(usage.tokens);
  const valid = draft.period === key.policy.period && draft.rule_mode === key.policy.rule_mode &&
    JSON.stringify(rule) === JSON.stringify(key.policy.channel_rules[channel.id]);
  const text = !valid ? "保存后刷新周期用量" : money && usage.unpriced ? "待计价" :
    used.toLocaleString("zh-CN", { maximumFractionDigits: 6 }) + (money ? " USD" : " Token");
  const usageCell = el("td", {}, el("div", { class: "usage-text" }, text));
  if (valid && quota && !(money && usage.unpriced)) {
    const exhausted = used >= Number(quota.limit);
    const fill = el("span"); fill.style.width = (Number(quota.limit) > 0 ? Math.min(100, used / Number(quota.limit) * 100) : 100) + "%";
    usageCell.append(el("div", { class: "progress" + (exhausted ? " exhausted" : "") }, fill),
      el("div", { class: "usage-note" }, exhausted ? "渠道额度已耗尽" : "剩余 " + Math.max(0, Number(quota.limit) - used).toLocaleString("zh-CN", { maximumFractionDigits: 6 })));
  }
  if (valid) {
    usageCell.append(el("div", { class: "usage-note" }, date(usage.reset_at) + " 重置"));
    if (usage.pending) usageCell.append(el("div", { class: "usage-note" }, usage.pending + " 个请求待结算"));
    if (usage.review) usageCell.append(el("button", { class: "review-action", onclick: () => guard(() => reviewModel("", channel.id)) }, "处理待核对（" + usage.review + "）"));
  }
  return el("tr", {},
    el("td", {}, el("strong", {}, channel.label), el("details", { class: "channel-models" },
      el("summary", {}, "模型渠道 · " + models.length + " 个已知模型"), el("ul", {}, models.length ? models.map((model) => el("li", {}, model.id)) : el("li", {}, "暂未发现模型，可预先配置规则")))),
    el("td", {}, access),
    el("td", {}, quota ? [el("strong", { class: "quota-value" }, Number(quota.limit).toLocaleString("zh-CN", { maximumFractionDigits: 6 })),
      el("span", { class: "model-family" }, (money ? "USD" : "Token") + " / " + ({ day: "每天", week: "每周", month: "每月" })[draft.period])] :
      el("span", { class: "muted" }, rule?.access === "deny" ? "禁止调用" : draft.total_quota_enabled ? "仅受总额度限制" : "不限额")),
    usageCell, el("td", { class: "row-actions" },
      el("button", { class: "secondary", disabled: !key.active, "data-quota-channel": channel.id, "aria-label": channel.label + " 渠道额度",
        onclick: () => editQuota(channel.id, true) }, quota ? "编辑额度" : "设置额度"),
      el("button", { class: "ghost danger-text", disabled: !key.active, onclick: () => { delete draft.channel_rules[channel.id]; markDirty(); renderDetail(); } }, "移除")));
}
function editQuota(model = null, channel = false, enableTotal = false) {
  const total = model === null, key = chosen(), ruleField = channel ? "channel_rules" : "rules", existing = total ? null : draft[ruleField][model];
  const label = total ? "密钥总额度" : channel ? (snapshot.channels.find((item) => item.id === model).label + " 渠道额度") : "单模型额度";
  const quota = total ? draft.total_quota : existing?.quota, usage = total ? key.total_usage : channel ? key.channel_usage[model] : key.quotas[model];
  const price = snapshot.models.find((item) => item.id === model)?.price;
  let unit = quota?.unit || (total ? "tokens" : "none");
  const amounts = { tokens: unit === "tokens" ? quota?.limit || "" : "", money: unit === "money" ? quota?.limit || "" : "" };
  const amount = numericField({ value: quota?.limit || "", "aria-label": "额度上限", placeholder: "输入额度上限" }, unit);
  const amountLabel = el("span");
  const amountField = field("", el("div", {}, amountLabel, amount.input, amount.message));
  amountField.className = "quota-amount-field";
  const help = el("p", { class: "fine" });
  const problem = el("p", { class: "quota-warning", hidden: true });
  const limitWarning = el("p", { class: "quota-warning", hidden: true });
  const updateLimitWarning = () => {
    const active = total ? enableTotal || draft.total_quota_enabled : existing?.access !== "deny";
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
    const unavailable = !total && !channel && unit === "money" && !price && existing?.access !== "deny";
    problem.hidden = !unavailable && existing?.access !== "deny" && !((total || channel) && unit === "money");
    problem.textContent = (total || channel) ? (existing?.access === "deny" ? "该渠道已禁止调用；保存额度不会开放权限。 " : "") + "生效的金额额度要求模型已设置价格；历史未计价用量需确认补计。"
      : unavailable ? "该模型尚未设置价格。请先在“模型价格”页补全价格，或使用 Token 额度。" : "该模型当前禁止调用；保存额度不会开放模型权限。";
    $("dialog-confirm").disabled = unavailable;
    usedText.textContent = draft.period !== key.policy.period || !usage ? "保存后计算" :
      unit === "money" ? (usage.unpriced ? "待计价" : Number(usage.cost).toLocaleString("zh-CN", { maximumFractionDigits: 6 }) + " USD") : Number(usage.tokens).toLocaleString("zh-CN") + " Token";
    updateLimitWarning();
  };
  for (const [value, label] of (total ? [["tokens", "Token"], ["money", "金额 USD"]] : [["none", "不限额"], ["tokens", "Token"], ["money", "金额 USD"]])) {
    modes.append(el("label", {}, el("input", { type: "radio", name: "quota-unit", value, onchange: () => {
      if (unit !== "none") amounts[unit] = amount.input.value;
      unit = value; amount.input.value = amounts[unit] || ""; dialogDirty = true; update();
    } }), el("span", {}, label)));
  }
  const consent = el("input", { type: "checkbox" });
  const consentField = field("按当前单价补计本周期未计价用量", consent); consentField.className = "reprice-consent"; consentField.hidden = true;
  const contents = [el("p", { class: total ? "" : "model-name" }, total ? "当前密钥 · 全部模型共享" : channel ? label : model), context, el("div", {}, el("p", { class: "field-label" }, "额度类型"), modes), amountField, help, problem, limitWarning, consentField];
  if (total) contents.push(el("p", { class: "fine" }, (enableTotal || draft.total_quota_enabled ? "保存后总额度启用。" : "保存后总额度仍关闭。") + "汇总本周期已记录用量；更早未记录的请求不会补算。"));
  if (dirty) contents.push(el("p", { class: "quota-warning" }, "本次保存会同时提交密钥草稿：备注「" + draft.note + "」、" +
    ({ day: "每天", week: "每周", month: "每月" })[draft.period] + "周期、按" + modeLabel(draft.rule_mode) + "规则、总额度" +
    (draft.total_quota_enabled || enableTotal ? "开启" : "关闭") + "，以及已修改的权限。"));
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
  }, enableTotal ? "保存并启用" : "保存额度");
  update();
}
function addRestriction(initialScope = "channel") {
  const key = chosen();
  let scope = initialScope, unit = scope === "all" ? "tokens" : "none";
  const amounts = { tokens: "", money: "" };
  const scopeModes = el("div", { class: "quota-modes scope-modes", role: "radiogroup", "aria-label": "限制范围" });
  const quotaModes = el("div", { class: "quota-modes", role: "radiogroup", "aria-label": "额度类型" });
  const firstNewChannel = snapshot.channels.find((item) => !draft.channel_rules[item.id]) || snapshot.channels[0];
  const channelSelect = select(snapshot.channels.map((item) => [item.id, item.label]), firstNewChannel?.id || "other");
  channelSelect.setAttribute("aria-label", "选择渠道");
  const modelInput = el("input", { placeholder: "输入或选择实际模型 ID", maxLength: 256 });
  modelInput.setAttribute("list", "restriction-models");
  modelInput.setAttribute("aria-label", "实际模型");
  const modelList = el("datalist", { id: "restriction-models" },
    snapshot.models.filter((model) => !model.targets?.length).map((model) => el("option", { value: model.id })));
  const accessSelect = select([["allow", "允许"], ["deny", "禁止"]], "allow");
  accessSelect.setAttribute("aria-label", "权限");
  const channelField = field("选择渠道", channelSelect);
  const modelField = field("实际模型", el("div", {}, modelInput, modelList));
  const accessField = field("权限", accessSelect);
  const amount = numericField({ "aria-label": "额度上限", placeholder: "输入额度上限" }, unit);
  const amountLabel = el("span");
  const amountField = field("", el("div", {}, amountLabel, amount.input, amount.message));
  amountField.className = "quota-amount-field";
  const help = el("p", { class: "fine" });
  const modeWarning = el("p", { class: "quota-warning", hidden: true });
  const priceWarning = el("p", { class: "quota-warning", hidden: true });
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
      dialogDirty = true; loadSelection();
    } }), el("span", {}, label)));
  }
  for (const [value, label] of [["none", "不限额"], ["tokens", "Token"], ["money", "金额 USD"]]) {
    quotaModes.append(el("label", { "data-quota-option": value }, el("input", { type: "radio", name: "restriction-unit", value, onchange: () => {
      if (unit !== "none") amounts[unit] = amount.input.value;
      unit = value; amount.input.value = amounts[unit] || ""; dialogDirty = true; update();
    } }), el("span", {}, label)));
  }
  const target = () => scope === "channel" ? channelSelect.value : modelInput.value.trim();
  const existing = () => scope === "all" ? (draft.total_quota ? { access: "allow", quota: draft.total_quota } : null) :
    draft[scope === "channel" ? "channel_rules" : "rules"][target()];
  const loadSelection = () => {
    const current = existing();
    accessSelect.value = current?.access || "allow";
    unit = current?.quota?.unit || (scope === "all" ? "tokens" : "none");
    amounts.tokens = current?.quota?.unit === "tokens" ? current.quota.limit : "";
    amounts.money = current?.quota?.unit === "money" ? current.quota.limit : "";
    amount.input.value = unit === "none" ? "" : amounts[unit];
    update();
  };
  const update = () => {
    [...scopeModes.querySelectorAll("input")].forEach((node) => { node.checked = node.value === scope; node.parentElement.classList.toggle("active", node.checked); });
    const denied = scope !== "all" && accessSelect.value === "deny";
    if ((scope === "all" || denied) && unit === "none" && scope === "all") unit = "tokens";
    if (denied && unit !== "none") { amounts[unit] = amount.input.value; unit = "none"; }
    quotaModes.querySelector('[data-quota-option="none"]').hidden = scope === "all";
    quotaModes.hidden = denied;
    amountField.hidden = denied || unit === "none";
    channelField.hidden = scope !== "channel";
    modelField.hidden = scope !== "model";
    accessField.hidden = scope === "all";
    modelInput.required = scope === "model";
    [...quotaModes.querySelectorAll("input")].forEach((node) => { node.checked = node.value === unit; node.parentElement.classList.toggle("active", node.checked); });
    amount.input.disabled = denied || unit === "none";
    amount.input.required = !denied && unit !== "none";
    amount.input.dataset.unit = unit;
    amount.input.inputMode = unit === "tokens" ? "numeric" : "decimal";
    amountLabel.textContent = unit === "tokens" ? "Token 上限" : "金额上限（USD）";
    const prefix = existing() ? "该范围已有设置，本次保存会更新它。 " : "";
    help.textContent = prefix + (scope === "all" ? "全部获准模型共享此额度。" :
      denied ? "禁止规则无需额度；保存后该范围的新请求将被拒绝。" :
      unit === "none" ? "只添加允许权限，不设置独立额度。" :
      scope === "channel" ? "渠道内所有实际模型共享额度；金额按各模型价格汇总。" : "只限制这个实际模型。");
    const targetMode = scope === "channel" ? "channel" : scope === "model" ? "model" : draft.rule_mode;
    const switching = scope !== "all" && targetMode !== draft.rule_mode && Object.keys(activeRules(draft)).length > 0;
    modeWarning.hidden = !switching && !dirty;
    modeWarning.textContent = switching
      ? "保存后将切换为按" + modeLabel(targetMode) + "限制；当前 " + Object.keys(activeRules(draft)).length + " 条" + modeLabel(draft.rule_mode) + "限制会保留但暂停。"
      : dirty ? "本次保存会一并提交当前密钥中尚未保存的备注、周期、开关或限制修改。" : "";
    const model = scope === "model" ? snapshot.models.find((item) => item.id === target()) : null;
    const missingPrice = scope === "model" && unit === "money" && accessSelect.value === "allow" && !model?.price;
    priceWarning.hidden = !missingPrice && unit !== "money";
    priceWarning.textContent = missingPrice
      ? "该模型尚未设置价格，请先在“模型价格”页补全价格，或改用 Token。"
      : unit === "money" ? "生效的金额额度要求相关模型已有价格；历史未计价用量可能需要确认补计。" : "";
    $("dialog-confirm").disabled = missingPrice;
  };
  channelSelect.onchange = loadSelection; modelInput.oninput = loadSelection; accessSelect.onchange = update;
  amount.input.addEventListener("input", () => { if (unit !== "none") amounts[unit] = amount.input.value; });
  modal("添加限制", [
    el("div", {}, el("p", { class: "field-label" }, "限制范围"), scopeModes),
    channelField, modelField, accessField,
    el("div", {}, el("p", { class: "field-label" }, "额度类型"), quotaModes),
    amountField, help, modeWarning, priceWarning, consentField,
  ], async () => {
    const candidate = structuredClone(draft);
    if (scope === "all") {
      candidate.total_quota_enabled = true;
      candidate.total_quota = { unit, limit: amount.input.value.trim() };
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
  }, "添加限制");
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
    if (error.code === "reprice_required") modal("确认补计历史费用", [el("p", {}, error.message)], () => savePolicy(true), "补计并保存");
    else notify(error.message, true);
  }
  finally { savingPolicy = false; restore(); $("save-policy").disabled = !dirty || !chosen()?.active; }
};
$("key-note").oninput = () => { draft.note = $("key-note").value; markDirty(); };
$("total-quota-enabled").onchange = () => {
  if ($("total-quota-enabled").checked && !draft.total_quota) {
    $("total-quota-enabled").checked = false; addRestriction("all"); return;
  }
  draft.total_quota_enabled = $("total-quota-enabled").checked; markDirty(); renderDetail();
};
$("key-period").onchange = () => { draft.period = $("key-period").value; markDirty(); renderDetail(); };
$("discard").onclick = () => confirmLeave(() => { draft = structuredClone(chosen().policy); dirty = false; renderDetail(); });
$("key-search").oninput = renderKeys;
$("add-restriction").onclick = () => addRestriction();
$("empty-add-restriction").onclick = () => addRestriction();
$("sync-models").onclick = () => guard(async () => {
  const restore = pendingButton($("sync-models"), "同步中…");
  try { const result = await api("/sync-models", "POST", { key_id: selected }); await load(true); notify("已同步 " + result.count + " 个模型"); }
  finally { restore(); $("sync-models").disabled = !chosen()?.active; }
});
function renderPrices(preserveFocus = false) {
  const models = snapshot.models.filter((m) => !m.targets?.length);
  const search = $("price-search").value.trim().toLowerCase(), filter = $("price-filter").value;
  const visible = models.filter((model) => model.id.toLowerCase().includes(search) &&
    (filter === "all" || (filter === "missing" && !model.price) || (filter === "manual" && model.price_source === "manual") || (filter === "automatic" && model.price && model.price_source !== "manual")));
  $("price-count").textContent = "显示 " + visible.length + " / " + models.length + " 个模型";
  const sync = snapshot.price_sync || {}, priced = models.filter((model) => model.price).length;
  $("price-coverage").textContent = priced + " / " + models.length + " 已设置";
  $("sync-prices").disabled = !!sync.running;
  $("sync-prices").textContent = sync.running ? "正在补全…" : "同步缺失价格";
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
  $("price-rows").replaceChildren(...visible.map((model) => el("tr", {},
    el("td", { class: "price-name" }, el("strong", {}, model.id), el("span", { class: "model-family" }, group(model.id))),
    ...["input", "output", "cache_read", "cache_write"].map((name) => el("td", { class: "price-value" }, model.price ? (model.price[name] ?? "沿用输入") : "—")),
    el("td", {}, el("span", { class: "badge" + (model.price ? "" : " warn") }, model.price_source === "manual" ? "手动设置" : (model.price_source || "待设置")),
      model.price_synced_at ? el("small", { class: "price-origin", title: model.price_source_model || "" }, date(model.price_synced_at, false)) : null),
    el("td", {}, el("button", { class: "secondary", "data-model": model.id, onclick: () => editPrice(model) }, model.price ? "编辑" : "设置价格")))));
  if (!visible.length) $("price-rows").append(el("tr", {}, el("td", { colSpan: 7, class: "empty compact" },
    icon("models"), el("h2", {}, models.length ? "没有匹配的价格" : "暂无模型"),
    el("p", {}, models.length ? "调整模型名称或价格状态后重试。" : "请先在 API 密钥页面同步或添加模型。"),
    models.length ? el("button", { class: "secondary", onclick: () => { $("price-search").value = ""; $("price-filter").value = "all"; renderPrices(); } }, "清除筛选") : null)));
  schedulePriceRefresh();
}
$("price-search").oninput = renderPrices; $("price-filter").onchange = renderPrices;
function schedulePriceRefresh() {
  clearTimeout(priceTimer);
  if (!token || !(view === "prices" || snapshot?.price_sync?.running)) return;
  const currentToken = token;
  priceTimer = setTimeout(async () => {
    try {
      const data = await api("/state");
      if (token !== currentToken) return;
      snapshot = data; renderPrices(true);
      if (view === "keys" && !dirty && !$("dialog").open) renderDetail();
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
    const control = numericField({ inputMode: "decimal", value: model.price?.[name] || "", required: ["input", "output"].includes(name), placeholder: ["input", "output"].includes(name) ? "例如 1.000000" : "留空沿用输入价" }, "money", !["input", "output"].includes(name));
    fields[name] = control.input;
    grid.append(field(label + " · USD / 百万 Token", el("div", {}, control.input, control.message)));
  }
  modal("设置模型价格", [el("p", { class: "model-name" }, model.id), grid, el("p", { class: "fine" }, "保存为手动价格后，自动同步不会覆盖。新价格仅影响之后放行的请求。")], async () => {
    const price = Object.fromEntries(Object.entries(fields).map(([name, node]) => [name, node.value.trim() || null]));
    await api("/price", "PUT", { model: model.id, price, revision: model.revision }); await load(true); notify("模型价格已保存");
  }, "保存价格");
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
  }, "确认用量并恢复");
}
document.querySelectorAll("[data-view]").forEach((button) => button.onclick = () => confirmLeave(() => setView(button.dataset.view)));
$("credential-form").onsubmit = async (event) => {
  event.preventDefault(); $("credential-error").textContent = "";
  const managementKey = $("settings-management-key").value.trim();
  if (!managementKey) { $("credential-error").textContent = "请输入管理密钥"; return; }
  const remember = $("remember-management-key").checked;
  const restore = pendingButton($("save-credential"), "正在连接…");
  token = managementKey; credentialSource = remember ? "saved" : "temporary";
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
$("refresh").onclick = () => confirmLeave(async () => {
  if (!token) { setView("settings"); return; }
  const restore = pendingButton($("refresh"), "刷新中…");
  try { await load(); notify("数据已刷新"); } finally { restore(); }
});
window.addEventListener("beforeunload", (event) => { if (dirty || dialogDirty) { event.preventDefault(); event.returnValue = ""; } });

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
