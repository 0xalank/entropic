import { invoke } from "@tauri-apps/api/core";

type NativeApiRequest = {
  method: string;
  url: string;
  accessToken?: string | null;
  body?: unknown;
  deviceFingerprint?: string | null;
};

type NativeApiResponse = {
  status: number;
  body: any;
};

type NativeApiUploadRequest = {
  url: string;
  accessToken: string;
  fileName: string;
  contentType: string;
  base64: string;
  deviceFingerprint?: string | null;
};

export function shouldUseNativeApiTransport(apiUrl: string): boolean {
  if (!apiUrl || (import.meta as any).env?.DEV) return false;
  if (!/^https?:\/\//i.test(apiUrl)) return false;
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function nativeApiRequest(
  request: NativeApiRequest
): Promise<NativeApiResponse> {
  return invoke<NativeApiResponse>("entropic_api_request_native", {
    request,
  });
}

export async function nativeApiUpload(
  request: NativeApiUploadRequest
): Promise<NativeApiResponse> {
  return invoke<NativeApiResponse>("entropic_api_upload_native", {
    request,
  });
}
