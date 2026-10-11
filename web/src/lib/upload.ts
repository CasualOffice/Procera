import { ApiError, api } from './api';

export interface FileInfo { id: string; name: string; size: number; received: number; state: string; sniffed_type: string | null; scan_result: string | null; sha256: string | null; uploaded_by: string; created_at: string; last_accessed_at?: string | null }

const CHUNK = 2 * 1024 * 1024;

/**
 * Uploads a file in chunks (FR-F01): if a chunk fails, it asks the server how much arrived and
 * resumes from there (up to 3 retries per chunk), then completes the upload (hash, type, scan).
 */
export async function uploadFile(file: File, onProgress?: (fraction: number) => void): Promise<FileInfo> {
  const created = await api<{ file: FileInfo }>('/api/v1/files', { method: 'POST', body: { name: file.name, size: file.size, content_type: file.type || null } });
  let info = created.file;
  let offset = 0;
  let failures = 0;
  while (offset < file.size) {
    try {
      info = await api<FileInfo>(`/api/v1/files/${info.id}/content?offset=${offset}`, { method: 'PUT', raw: file.slice(offset, offset + CHUNK) });
      offset = info.received;
      failures = 0;
      onProgress?.(offset / file.size);
    } catch (e) {
      if (e instanceof ApiError && e.status !== 409 && e.status < 500) throw e; // a real refusal, not a lost chunk
      if (++failures > 3) throw e;
      // Resume from what the server has.
      offset = (await api<{ file: FileInfo }>(`/api/v1/files/${info.id}`)).file.received;
    }
  }
  return api<FileInfo>(`/api/v1/files/${info.id}/complete`, { method: 'POST' });
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(1)} GB`;
}

/** Downloads a clean file through a short-lived signed link. */
export async function downloadFile(id: string): Promise<void> {
  const { url } = await api<{ url: string }>(`/api/v1/files/${id}/download-url`);
  const a = document.createElement('a');
  a.href = url;
  a.rel = 'noopener';
  a.click();
}
