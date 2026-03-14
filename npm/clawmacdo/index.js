"use strict";

const path = require("path");

const PLATFORM_PACKAGES = {
  "darwin-arm64": "@clawmacdo/darwin-arm64",
  "linux-x64": "@clawmacdo/linux-x64",
  "win32-x64": "@clawmacdo/win32-x64",
};

/**
 * Returns the absolute path to the clawmacdo binary for the current platform.
 * Throws if the current platform is unsupported or the platform package is missing.
 */
function getBinaryPath() {
  const platformKey = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[platformKey];

  if (!pkg) {
    const supported = Object.keys(PLATFORM_PACKAGES)
      .map((k) => k.replace("-", " "))
      .join(", ");
    throw new Error(
      `clawmacdo does not support ${process.platform} ${process.arch}. ` +
        `Supported platforms: ${supported}`
    );
  }

  const binName = process.platform === "win32" ? "clawmacdo.exe" : "clawmacdo";

  try {
    return require.resolve(`${pkg}/bin/${binName}`);
  } catch {
    throw new Error(
      `The platform package ${pkg} is not installed. ` +
        `Try reinstalling clawmacdo: npm install clawmacdo`
    );
  }
}

module.exports = { getBinaryPath, PLATFORM_PACKAGES };
