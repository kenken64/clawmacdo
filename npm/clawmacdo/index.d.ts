/**
 * Returns the absolute path to the clawmacdo binary for the current platform.
 * @throws If the current platform is unsupported or the platform package is missing.
 */
export function getBinaryPath(): string;

/**
 * Map of platform keys (e.g. "darwin-arm64") to npm package names.
 */
export const PLATFORM_PACKAGES: Record<string, string>;
