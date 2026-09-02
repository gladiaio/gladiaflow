import { invoke } from "@tauri-apps/api/core";

export const CONFIG_LOAD_ERROR_MESSAGE =
  "GladiaFlow couldn't read your saved settings. Your API key has not been changed.";
export const CONFIG_RESET_ERROR_MESSAGE =
  "GladiaFlow couldn't reset your settings. Your original settings file was not deleted.";

export type ApiKeyLoadResult =
  | { ok: true; apiKey: string | null }
  | { ok: false; message: string };

export async function loadSavedApiKey(
  invokeApiKey: () => Promise<string | null> = () =>
    invoke<string | null>("get_api_key"),
): Promise<ApiKeyLoadResult> {
  try {
    return { ok: true, apiKey: await invokeApiKey() };
  } catch {
    return { ok: false, message: CONFIG_LOAD_ERROR_MESSAGE };
  }
}

export type ConfigResetResult =
  | { ok: true; backupPath: string }
  | { ok: false; message: string };

export async function resetCorruptedConfig(
  invokeReset: (confirmed: boolean) => Promise<string> = (confirmed) =>
    invoke<string>("reset_corrupted_config", { confirmed }),
): Promise<ConfigResetResult> {
  try {
    return { ok: true, backupPath: await invokeReset(true) };
  } catch {
    return { ok: false, message: CONFIG_RESET_ERROR_MESSAGE };
  }
}
