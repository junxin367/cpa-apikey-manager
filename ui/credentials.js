"use strict";
(function (root) {
  const PREFIX = "enc::v1::";
  const HOST_AUTH_KEY = "cli-proxy-auth";
  const LEGACY_HOST_KEY = "managementKey";
  const PLUGIN_KEY = "cpa-apikey-manager-management-key";
  const HOST_SALT = "cli-proxy-api-webui::secure-storage";
  const PLUGIN_SALT = "cpa-apikey-manager::management-key";

  function keyBytes(salt, host, userAgent) {
    return new TextEncoder().encode(salt + "|" + host + "|" + userAgent);
  }
  function transform(bytes, key) {
    const result = new Uint8Array(bytes.length);
    for (let index = 0; index < bytes.length; index++) result[index] = bytes[index] ^ key[index % key.length];
    return result;
  }
  function base64(bytes) {
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return btoa(binary);
  }
  function unbase64(value) {
    const binary = atob(value), bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
    return bytes;
  }
  function encode(value, salt, host, userAgent) {
    const bytes = new TextEncoder().encode(JSON.stringify(value));
    return PREFIX + base64(transform(bytes, keyBytes(salt, host, userAgent)));
  }
  function decode(value, salt, host, userAgent) {
    if (!value) return null;
    let text = value;
    if (value.startsWith(PREFIX)) {
      const bytes = transform(unbase64(value.slice(PREFIX.length)), keyBytes(salt, host, userAgent));
      text = new TextDecoder().decode(bytes);
    }
    try { return JSON.parse(text); } catch { return text; }
  }
  function read(storage, key, salt, host, userAgent) {
    try { return decode(storage.getItem(key), salt, host, userAgent); } catch { return null; }
  }
  function text(value) {
    return typeof value === "string" ? value.trim() : "";
  }
  function readHostManagementKey(storage, host, userAgent) {
    const persisted = read(storage, HOST_AUTH_KEY, HOST_SALT, host, userAgent);
    const current = text(persisted?.state?.managementKey || persisted?.managementKey);
    if (current) return current;
    return text(read(storage, LEGACY_HOST_KEY, HOST_SALT, host, userAgent));
  }
  function readSavedManagementKey(storage, host, userAgent) {
    const saved = read(storage, PLUGIN_KEY, PLUGIN_SALT, host, userAgent);
    return text(saved?.managementKey);
  }
  function saveManagementKey(storage, managementKey, host, userAgent) {
    const value = text(managementKey);
    if (!value) throw new Error("管理密钥不能为空");
    storage.setItem(PLUGIN_KEY, encode({ managementKey: value }, PLUGIN_SALT, host, userAgent));
  }
  function clearManagementKey(storage) {
    storage.removeItem(PLUGIN_KEY);
  }

  root.CpaCredentials = Object.freeze({
    readHostManagementKey,
    readSavedManagementKey,
    saveManagementKey,
    clearManagementKey,
  });
})(globalThis);
