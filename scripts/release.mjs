import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const platforms = [
  ["windows-x64", "dll"],
  ["linux-x64", "so"],
  ["macos-x64", "dylib"],
  ["macos-arm64", "dylib"],
];
const semver = /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
const assetsFor = (version) => platforms.flatMap(([platform, extension]) =>
  [`cpa-apikey-manager-${version}-${platform}.zip`, `cpa-apikey-manager-${version}-${platform}.${extension}`]);

function versionFromTag(tag) {
  const version = tag.replace(/^[vV]/, "");
  if (!/^[vV]/.test(tag) || !semver.test(version)) {
    throw new Error(`无效版本标签：${tag}，请使用 v0.1.0 格式`);
  }
  return version;
}

function packageVersion() {
  const text = readFileSync("Cargo.toml", "utf8");
  const section = text.split(/^\[package\][ \t]*\r?$/m)[1]?.split(/^\[/m)[0];
  const version = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error("无法读取 Cargo.toml 的包版本");
  return version;
}

function metadata() {
  const tag = process.env.RELEASE_TAG || "";
  const version = packageVersion();
  const sha = git("rev-parse", "--verify", "HEAD");
  if (tag) {
    if (versionFromTag(tag) !== version) {
      throw new Error(`标签 ${tag} 与 Cargo.toml 版本 ${version} 不一致`);
    }
    if (git("rev-parse", "--verify", `refs/tags/${tag}^{commit}`) !== sha) {
      throw new Error("当前代码提交与发布标签不一致");
    }
  }
  const output = `tag=${tag}\nversion=${version}\nsha=${sha}\n`;
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, output);
  process.stdout.write(output);
}

function notes(tag, repo) {
  const version = versionFromTag(tag);
  let previous = "";
  try {
    previous = git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", "--match", "V[0-9]*", `${tag}^`);
  } catch {
    // 首次发布没有上一个版本。
  }
  const changes = git("log", "--format=%s", previous ? `${previous}..${tag}` : tag)
    .split(/\r?\n/).filter(Boolean).map((subject) => `- ${subject}`);
  const base = `https://github.com/${repo}`;
  const lines = [
    `## ${tag}`, "", `CPA API 密钥权限与额度管理 ${version}。`, "",
    "### 变更", "", ...changes, "", "### 下载与安装", "",
    ...platforms.map(([platform]) => `- ${platform}：\`cpa-apikey-manager-${version}-${platform}.zip\``),
    "", "解压 ZIP，将其中的插件动态库放入 CLIProxyAPI 的 plugins 目录，再按 README 配置。",
    "也可以下载独立动态库，安装时重命名为 cpa-apikey-manager.dll、cpa-apikey-manager.so 或 cpa-apikey-manager.dylib。",
    "", "`checksums.txt` 包含所有 ZIP 和独立动态库的 SHA-256；`latest.json` 在全部平台产物上传后生成。",
    "四个平台由 GitHub Actions 构建；宿主功能验收范围以仓库验证记录为准。",
    "", `[安装说明](${base}/blob/${tag}/README.md) · [验证记录](${base}/blob/${tag}/docs/verification.md)`,
    "", previous ? `[完整变更](${base}/compare/${previous}...${tag})` : `[版本提交](${base}/commits/${tag})`, "",
  ];
  process.stdout.write(lines.join("\n"));
}

function checksums(directory, version) {
  const expected = assetsFor(version).sort();
  const actual = readdirSync(directory).filter((name) => name !== "checksums.txt" && name !== "latest.json").sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`发布产物不完整或包含额外文件。\n预期：${expected.join(", ")}\n实际：${actual.join(", ")}`);
  }
  const lines = expected.map((name) => {
    const bytes = readFileSync(join(directory, name));
    if (bytes.length === 0) throw new Error(`发布产物为空：${name}`);
    return `${createHash("sha256").update(bytes).digest("hex")}  ${name}`;
  });
  writeFileSync(join(directory, "checksums.txt"), `${lines.join("\n")}\n`);
}

function manifest(input, output) {
  const release = JSON.parse(readFileSync(input, "utf8"));
  const version = versionFromTag(release.tagName);
  const assets = (release.assets || []).filter((asset) => asset.name !== "latest.json");
  for (const expected of [...assetsFor(version), "checksums.txt"]) {
    if (!assets.some((asset) => asset.name === expected)) throw new Error(`Release 缺少产物：${expected}`);
  }
  const payload = {
    version: `v${version}`,
    url: release.url,
    body: release.body || "",
    assets: assets.map((asset) => ({ name: asset.name, url: asset.url }))
      .sort((left, right) => left.name.localeCompare(right.name)),
  };
  if (!payload.url || payload.assets.some((asset) => !asset.url)) throw new Error("Release 缺少下载地址");
  writeFileSync(output, `${JSON.stringify(payload, null, 2)}\n`);
}

const [command, ...args] = process.argv.slice(2);
switch (command) {
  case "metadata": metadata(); break;
  case "notes": notes(...args); break;
  case "checksums": checksums(...args); break;
  case "manifest": manifest(...args); break;
  default: throw new Error("用法：node scripts/release.mjs metadata|notes|checksums|manifest");
}
