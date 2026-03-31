#!/usr/bin/env node
import { readFileSync, writeFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");

const tauriConfPath = resolve(root, "src-tauri/tauri.conf.json");
const packageJsonPath = resolve(root, "package.json");
const cargoTomlPath = resolve(root, "src-tauri/Cargo.toml");

const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf-8"));
const cargoToml = readFileSync(cargoTomlPath, "utf-8");

// If a version arg is provided, use it. Otherwise sync from tauri.conf.json.
const newVersion = process.argv[2] || tauriConf.version;

if (!newVersion) {
  console.error("No version found. Pass a version argument: bun run bump 0.2.0");
  process.exit(1);
}

// Validate semver format (basic check)
if (!/^\d+\.\d+\.\d+/.test(newVersion)) {
  console.error(`Invalid version format: "${newVersion}". Expected semver (e.g. 0.2.0)`);
  process.exit(1);
}

// 1. Update tauri.conf.json
tauriConf.version = newVersion;
writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n");
console.log(`  tauri.conf.json  -> ${newVersion}`);

// 2. Update package.json
packageJson.version = newVersion;
writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + "\n");
console.log(`  package.json     -> ${newVersion}`);

// 3. Update Cargo.toml (only the package version, not dependency versions)
const updatedCargo = cargoToml.replace(
  /^(version\s*=\s*)"[^"]*"/m,
  `$1"${newVersion}"`
);
writeFileSync(cargoTomlPath, updatedCargo);
console.log(`  Cargo.toml       -> ${newVersion}`);

console.log(`\nVersion bumped to ${newVersion}`);
