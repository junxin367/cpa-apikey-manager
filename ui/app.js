"use strict";
const API = "/v0/management/plugins/cpa-apikey-manager";
const $ = (id) => document.getElementById(id);
let token = "", snapshot = null, selected = "", draft = null, dirty = false, family = "全部";
let view = "keys", toastTimer, priceTimer, dialogAction, busy = false;
let dialogDirty = false, dialogTrigger = null, fieldSequence = 0, savingPolicy = false;
let priceRowsSignature = "";

function icon(name) {
  const paths = {
    shield: ["M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6l-8-3Z", "m8 12 3 3 5-6"],
    key: ["M14 7a4 4 0 1 1-2 7L6 20H3v-3l6-6a4 4 0 0 1 5-4Z", "M16 8h.01"],
    coins: ["M17 7c0 2-3 3-7 3S3 9 3 7s3-3 7-3 7 1 7 3Z", "M3 7v5c0 2 3 3 7 3", "M3 12v5c0 2 3 3 7 3", "M17 10c3 0 4 1 4 3s-2 3-5 3-5-1-5-3 2-3 5-3", "M11 13v5c0 2 2 3 5 3s5-1 5-3v-5"],
    models: ["M3 3h7v7H3Z", "M14 3h7v7h-7Z", "M3 14h7v7H3Z", "M14 14h7v7h-7Z"],
    clock: ["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z", "M12 7v5l3 2"],
    refresh: ["M20 7v5h-5", "M4 17v-5h5", "M6 7a7 7 0 0 1 12-1l2 6", "M18 17a7 7 0 0 1-12 1l-2-6"],
    logout: ["M10 4H4v16h6", "M9 12h12", "m17 8 4 4-4 4"],
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
    headers: { Authorization: "Bearer " + token, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(20000),
  });
  let data;
  try { data = await response.json(); } catch { throw new Error("宿主返回了无效响应，请检查插件状态"); }
  if (!response.ok) {
    const error = new Error(data.error?.message || (response.status === 401 ? "管理密钥无效" : "请求失败：" + response.status));
    error.code = data.error?.code; throw error;
  }
  return data;
}
async function guard(action) { try { await action(); } catch (error) { notify(error.message, true); } }
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
  else if (dialogTrigger?.dataset.quotaModel) [...$("model-rows").querySelectorAll("button")].find((node) => node.dataset.quotaModel === dialogTrigger.dataset.quotaModel)?.focus({ preventScroll: true });
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
  const name = id.toLowerCase();
  if (/gpt|codex|^o[1-9](?:-|$)/.test(name)) return "GPT";
  if (/claude/.test(name)) return "Claude";
  if (/deepseek/.test(name)) return "DeepSeek";
  if (/glm/.test(name)) return "GLM";
  if (/kimi|moonshot/.test(name)) return "Kimi";
  return "其他";
}
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
  $("key-count").textContent = snapshot.keys.filter((k) => k.active).length;
  $("stat-keys").textContent = $("key-count").textContent;
  $("stat-models").textContent = snapshot.models.filter((model) => !model.targets?.length).length;
  $("stat-rules").textContent = snapshot.keys.reduce((count, key) => count + Object.keys(key.policy.rules).length, 0);
  const problem = snapshot.source_error || snapshot.health_error;
  $("system-error").textContent = problem ? "当前请求受到保护性拦截：" + problem : "";
  $("system-error").hidden = !problem;
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
  el("span", { class: "key-meta" }, el("small", {}, Object.keys(key.policy.rules).length + " 个模型规则"), el("span", { class: "badge" + (key.active ? "" : " danger") }, key.active ? "有效" : "已删除")))));
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
  $("sync-models").disabled = !key.active;
  $("family-filters").replaceChildren(...["全部", "GPT", "Claude", "DeepSeek", "GLM", "Kimi", "其他"].map((name) => el("button", {
    class: name === family ? "active" : "", "aria-pressed": name === family, text: name,
    onclick: () => { family = name; renderDetail(); },
  })));
  const search = $("model-search").value.trim().toLowerCase();
  const ids = [...new Set([...snapshot.models.map((m) => m.id), ...Object.keys(draft.rules)])];
  // A unique alias shares one canonical row and one quota with its target.
  const models = ids.filter((id) => !snapshot.models.find((m) => m.id === id)?.targets?.length || snapshot.models.find((m) => m.id === id).targets.length > 1)
    .filter((id) => (family === "全部" || group(id) === family) && id.toLowerCase().includes(search)).sort();
  const focused = document.activeElement?.getAttribute("aria-label");
  $("model-rows").replaceChildren(...models.map(modelRow));
  if (focused) [...$("model-rows").querySelectorAll("[aria-label]")].find((node) => node.getAttribute("aria-label") === focused)?.focus({ preventScroll: true });
  $("no-models").hidden = models.length !== 0;
  if (!models.length) {
    const filtered = search || family !== "全部";
    $("no-models").replaceChildren(icon("models"), el("h2", {}, filtered ? "没有匹配的模型" : "暂无模型"),
      el("p", {}, filtered ? "调整搜索内容或模型分组后重试。" : "同步宿主模型目录，或添加实际模型 ID。"),
      filtered ? el("button", { class: "secondary", onclick: () => { family = "全部"; $("model-search").value = ""; renderDetail(); } }, "清除筛选") : null);
  }
}
function ensureRule(model) {
  if (!draft.rules[model]) draft.rules[model] = { access: "allow", quota: null };
  return draft.rules[model];
}
function modelRow(model) {
  const key = chosen(), rule = draft.rules[model];
  const aliases = snapshot.models.filter((m) => m.targets?.length === 1 && m.targets[0] === model).map((m) => m.id);
  const access = select([["inherit", "未配置"], ["allow", "允许"], ["deny", "禁止"]], rule?.access || "inherit", (e) => {
    if (e.target.value === "inherit") delete draft.rules[model];
    else ensureRule(model).access = e.target.value;
    markDirty(); renderDetail();
  }); access.setAttribute("aria-label", model + " 权限"); access.disabled = !key.active;
  const quotaCell = el("td", {}, rule?.quota
    ? [el("strong", { class: "quota-value" }, Number(rule.quota.limit).toLocaleString("zh-CN", { maximumFractionDigits: 6 })),
      el("span", { class: "model-family" }, (rule.quota.unit === "tokens" ? "Token" : "USD") + " / " + ({ day: "每天", week: "每周", month: "每月" })[draft.period])]
    : el("span", { class: "muted" }, "不限额"));
  const usage = key.quotas[model];
  const usageCell = el("td");
  if (rule?.quota && usage && draft.period === key.policy.period && JSON.stringify(rule.quota) === JSON.stringify(key.policy.rules[model]?.quota)) {
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
  } else usageCell.append(el("span", { class: "usage-note" }, rule?.quota ? "保存后刷新周期用量" : "—"));
  const action = el("button", { class: "secondary", "data-quota-model": model, "aria-label": model + (rule?.quota ? " 编辑额度" : " 设置额度"), disabled: !key.active, onclick: () => editQuota(model) }, rule?.quota ? "编辑额度" : "设置额度");
  return el("tr", {}, el("td", {}, el("span", { class: "model-name" }, model), el("span", { class: "model-family" }, group(model) + (aliases.length ? " · 别名：" + aliases.join(", ") : ""))), el("td", {}, access), quotaCell, usageCell, el("td", {}, action));
}
function editQuota(model) {
  const key = chosen(), existing = draft.rules[model], usage = key.quotas[model];
  const price = snapshot.models.find((item) => item.id === model)?.price;
  let unit = existing?.quota?.unit || "none";
  const amounts = { tokens: unit === "tokens" ? existing.quota.limit : "", money: unit === "money" ? existing.quota.limit : "" };
  const amount = numericField({ value: existing?.quota?.limit || "", "aria-label": "额度上限", placeholder: "输入额度上限" }, unit);
  const amountLabel = el("span");
  const amountField = field("", el("div", {}, amountLabel, amount.input, amount.message));
  amountField.className = "quota-amount-field";
  const help = el("p", { class: "fine" });
  const problem = el("p", { class: "quota-warning", hidden: true });
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
    help.textContent = unit === "none" ? "移除该模型的额度限制，保留当前权限。" : unit === "tokens" ? "按总 Token 限制用量。填写 0 将立即阻止该模型的新请求。" : "按模型价格计算消耗，最多 6 位小数。填写 0 将立即阻止新请求。";
    const unavailable = unit === "money" && !price;
    problem.hidden = !unavailable && existing?.access !== "deny";
    problem.textContent = unavailable ? "该模型尚未设置价格。请先在“模型价格”页补全价格，或使用 Token 额度。" : "该模型当前禁止调用；保存额度不会开放模型权限。";
    $("dialog-confirm").disabled = unavailable;
    usedText.textContent = draft.period !== key.policy.period || !usage ? "保存后计算" :
      unit === "money" ? (usage.unpriced ? "待计价" : Number(usage.cost).toLocaleString("zh-CN", { maximumFractionDigits: 6 }) + " USD") : Number(usage.tokens).toLocaleString("zh-CN") + " Token";
  };
  for (const [value, label] of [["none", "不限额"], ["tokens", "Token"], ["money", "金额 USD"]]) {
    modes.append(el("label", {}, el("input", { type: "radio", name: "quota-unit", value, onchange: () => {
      if (unit !== "none") amounts[unit] = amount.input.value;
      unit = value; amount.input.value = amounts[unit] || ""; dialogDirty = true; update();
    } }), el("span", {}, label)));
  }
  const consent = el("input", { type: "checkbox" });
  const consentField = field("按当前单价补计本周期未计价用量", consent); consentField.className = "reprice-consent"; consentField.hidden = true;
  const contents = [el("p", { class: "model-name" }, model), context, el("div", {}, el("p", { class: "field-label" }, "额度类型"), modes), amountField, help, problem, consentField];
  if (dirty) contents.push(el("p", { class: "fine" }, "本次保存会同时提交当前密钥的其他未保存修改。"));
  modal("配置模型额度", contents, async () => {
    const candidate = structuredClone(draft);
    if (unit === "none") { if (candidate.rules[model]) candidate.rules[model].quota = null; }
    else {
      candidate.rules[model] ||= { access: "allow", quota: null };
      candidate.rules[model].quota = { unit, limit: amount.input.value.trim() };
    }
    try {
      await api("/policy", "PUT", { key_id: selected, policy: candidate, reprice: consent.checked });
      dirty = false; await load(); notify("额度已保存，对新请求生效");
    } catch (error) {
      if (error.code === "reprice_required") { consentField.hidden = false; consent.required = true; }
      throw error;
    }
  }, "保存额度");
  update();
}
async function savePolicy(reprice = false) {
  await api("/policy", "PUT", { key_id: selected, policy: draft, reprice });
  dirty = false; await load(); notify("配置已保存，对新请求生效");
}
$("save-policy").onclick = async () => {
  if (savingPolicy) return;
  const invalidModel = Object.entries(draft.rules).find(([, rule]) => rule.quota && !(rule.quota.unit === "tokens" ? /^\d+$/ : /^\d+(?:\.\d{0,6})?$/).test(rule.quota.limit.trim()));
  if (invalidModel) { family = "全部"; $("model-search").value = invalidModel[0]; renderDetail(); }
  const valid = [...$("model-rows").querySelectorAll("input[data-unit]")].map(validateNumber).every(Boolean);
  if (!valid) { $("model-rows").querySelector('[aria-invalid="true"]')?.focus(); return; }
  savingPolicy = true; const restore = pendingButton($("save-policy"), "保存中…");
  try { await savePolicy(); }
  catch (error) {
    if (error.code === "reprice_required") modal("确认补计历史费用", [el("p", {}, error.message)], () => savePolicy(true), "补计并保存");
    else notify(error.message, true);
  }
  finally { savingPolicy = false; restore(); $("save-policy").disabled = !dirty || !chosen()?.active; }
};
$("key-note").oninput = () => { draft.note = $("key-note").value; markDirty(); };
$("key-period").onchange = () => { draft.period = $("key-period").value; markDirty(); renderDetail(); };
$("discard").onclick = () => confirmLeave(() => { draft = structuredClone(chosen().policy); dirty = false; renderDetail(); });
$("key-search").oninput = renderKeys; $("model-search").oninput = renderDetail;
$("sync-models").onclick = () => guard(async () => {
  const restore = pendingButton($("sync-models"), "同步中…");
  try { const result = await api("/sync-models", "POST", { key_id: selected }); await load(true); notify("已同步 " + result.count + " 个模型"); }
  finally { restore(); $("sync-models").disabled = !chosen()?.active; }
});
$("add-model").onclick = () => {
  const input = el("input", { required: true, placeholder: "输入实际模型 ID", maxLength: 256 });
  modal("添加模型", [field("模型 ID", input), el("p", { class: "fine" }, "请输入 CLIProxyAPI 中的模型 ID。添加目录记录不会创建上游模型。")], async () => {
    await api("/model", "POST", { model: input.value.trim() }); await load(true); notify("模型已添加");
  }, "添加");
};
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
async function reviewModel(model) {
  const result = await api("/reviews?" + new URLSearchParams({ key: selected, model }));
  if (!result.items.length) { await load(true); notify("该模型已无待核对请求"); return; }
  settle(result.items[0], result.items.length);
}
function settle(item, count) {
  const fields = {}, grid = el("div", { class: "field-grid" });
  for (const [name, label] of [["input", "普通输入 Token"], ["output", "输出 Token"], ["cache_read", "缓存读取 Token"], ["cache_write", "缓存写入 Token"]]) {
    fields[name] = el("input", { type: "number", min: "0", step: "1", max: "9000000000000000", required: true, value: "0" }); grid.append(field(label, fields[name]));
  }
  const reason = el("textarea", { required: true, minLength: 4, maxLength: 500, rows: 3, placeholder: "填写数据来源和核对依据" });
  modal("处理待核对", [el("p", {}, item.model + " · 尚有 " + count + " 个请求待处理"), el("p", { class: "fine" }, "当前请求：" + item.request_id), grid, field("核对原因", reason), el("p", { class: "fine" }, "全部核对后恢复该模型调用。之后收到真实用量时，将修正人工记录。")], async () => {
    const tokens = Object.fromEntries(Object.entries(fields).map(([key, node]) => [key, Number(node.value)]));
    if (Object.values(tokens).some((v) => !Number.isSafeInteger(v) || v < 0)) throw new Error("Token 必须为有效的非负整数");
    tokens.total = tokens.input + tokens.output + tokens.cache_read + tokens.cache_write; tokens.complete = true;
    await api("/settle", "POST", { request_id: item.request_id, model: item.model, tokens, reason: reason.value });
    await load(true); notify("已核对该请求");
  }, "确认用量并恢复");
}
document.querySelectorAll("[data-view]").forEach((button) => button.onclick = () => confirmLeave(async () => {
  view = button.dataset.view;
  if (chosen()) draft = structuredClone(chosen().policy);
  document.querySelectorAll("[data-view]").forEach((node) => { node.classList.toggle("active", node === button); if (node === button) node.setAttribute("aria-current", "page"); else node.removeAttribute("aria-current"); });
  for (const name of ["keys", "prices"]) $(name + "-view").hidden = name !== view;
  $("breadcrumb").textContent = ({ keys: "API 密钥", prices: "模型价格" })[view];
  schedulePriceRefresh();
  if (view === "keys") renderDetail();
}));
$("login-form").onsubmit = async (event) => {
  event.preventDefault(); $("login-error").textContent = "";
  const button = $("login-form").querySelector("button"), restore = pendingButton(button, "正在连接…");
  token = $("management-key").value.trim();
  try { await load(); $("management-key").value = ""; $("login").hidden = true; $("app").hidden = false; }
  catch (error) { token = ""; $("login-error").textContent = error.message; }
  finally { restore(); }
};
$("refresh").onclick = () => confirmLeave(async () => {
  const restore = pendingButton($("refresh"), "刷新中…");
  try { await load(); notify("数据已刷新"); } finally { restore(); }
});
$("logout").onclick = () => confirmLeave(() => {
  token = ""; snapshot = null; draft = null; selected = ""; dirty = false;
  clearTimeout(priceTimer);
  $("app").hidden = true; $("login").hidden = false; $("login-error").textContent = "";
});
window.addEventListener("beforeunload", (event) => { if (dirty || dialogDirty) { event.preventDefault(); event.returnValue = ""; } });
