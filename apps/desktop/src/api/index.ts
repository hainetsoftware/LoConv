import { isTauri } from '@tauri-apps/api/core';
import { DesktopApi } from './contracts';
import { MockDesktopApi } from './mock';
import { TauriDesktopApi } from './tauri';

export * from './contracts';
export * from './mock';
export * from './tauri';
export * from './validation';

let apiInstance: DesktopApi | null = null;

export function getDesktopApi(): DesktopApi {
  if (apiInstance) return apiInstance;

  const inTauri = typeof window !== 'undefined' && (isTauri() || '__TAURI_INTERNALS__' in window);
  const useMockParam =
    typeof window !== 'undefined' &&
    new URLSearchParams(window.location.search).get('mock') === 'true';

  if (inTauri && !useMockParam) {
    apiInstance = new TauriDesktopApi();
    return apiInstance;
  }

  // In production outside of Tauri, we must not silently fall back to mock
  if (import.meta.env.PROD && !useMockParam) {
    throw new Error(
      'LoConv production environment failure: Tauri native host runtime is unavailable. Synthetic mock adapter is disabled in release builds.',
    );
  }

  // In development or test, fall back to MockDesktopApi
  apiInstance = new MockDesktopApi();
  return apiInstance;
}

export function setDesktopApi(api: DesktopApi | null): void {
  apiInstance = api;
}
