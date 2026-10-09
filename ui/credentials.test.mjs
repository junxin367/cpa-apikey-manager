import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const host = "127.0.0.1:8317";
const userAgent = "credential-test";
const source = readFileSync(new URL("./credentials.js", import.meta.url), "utf8");
const context = { TextEncoder, TextDecoder, Uint8Array, atob, btoa };
context.globalThis = context;
vm.runInNewContext(source, context);
const credentials = context.CpaCredentials;

class Storage {
  values = new Map();
  getItem(key) { return this.values.get(key) ?? null; }
  setItem(key, value) { this.values.set(key, value); }
  removeItem(key) { this.values.delete(key); }
}

function encode(value, salt) {
  const prefix = "enc::v1::";
  const key = new TextEncoder().encode(`${salt}|${host}|${userAgent}`);
  const input = new TextEncoder().encode(JSON.stringify(value));
  const output = new Uint8Array(input.length);
  for (let index = 0; index < input.length; index++) output[index] = input[index] ^ key[index % key.length];
  return prefix + Buffer.from(output).toString("base64");
}

test("读取官方管理中心保存的管理密钥", () => {
  const storage = new Storage();
  storage.setItem("cli-proxy-auth", encode({
    state: { apiBase: "http://127.0.0.1:8317", managementKey: "host-secret", rememberPassword: true },
    version: 0,
  }, "cli-proxy-api-webui::secure-storage"));
  assert.equal(credentials.readHostManagementKey(storage, host, userAgent), "host-secret");
});

test("兼容旧管理中心单独保存的管理密钥", () => {
  const storage = new Storage();
  storage.setItem("managementKey", encode("legacy-secret", "cli-proxy-api-webui::secure-storage"));
  assert.equal(credentials.readHostManagementKey(storage, host, userAgent), "legacy-secret");
});

test("保存、读取和清除插件管理密钥", () => {
  const storage = new Storage();
  credentials.saveManagementKey(storage, "plugin-secret", host, userAgent);
  assert.match(storage.getItem("cpa-apikey-manager-management-key"), /^enc::v1::/);
  assert.equal(credentials.readSavedManagementKey(storage, host, userAgent), "plugin-secret");
  credentials.clearManagementKey(storage);
  assert.equal(credentials.readSavedManagementKey(storage, host, userAgent), "");
});

test("损坏的浏览器存储不会阻止页面启动", () => {
  const storage = new Storage();
  storage.setItem("cli-proxy-auth", "enc::v1::invalid");
  storage.setItem("cpa-apikey-manager-management-key", "{broken");
  assert.equal(credentials.readHostManagementKey(storage, host, userAgent), "");
  assert.equal(credentials.readSavedManagementKey(storage, host, userAgent), "");
});
