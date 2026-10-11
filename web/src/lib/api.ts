/** API client: bearer access token kept in memory only; renewed through the HttpOnly refresh cookie. */

export interface Problem {
  status: number;
  code: string;
  title: string;
  detail?: string;
}

export class ApiError extends Error {
  constructor(public problem: Problem) {
    super(problem.detail ? `${problem.title}: ${problem.detail}` : problem.title);
  }
  get code() {
    return this.problem.code;
  }
  get status() {
    return this.problem.status;
  }
}

let accessToken: string | null = null;
export const setAccessToken = (t: string | null) => {
  accessToken = t;
};

const CSRF = { 'X-Requested-With': 'procera' };

async function parse(res: Response) {
  if (res.status === 204) return undefined;
  const text = await res.text();
  const body = text ? JSON.parse(text) : undefined;
  if (!res.ok) throw new ApiError(body?.code ? body : { status: res.status, code: 'http_error', title: res.statusText });
  return body;
}

/**
 * Renews the access token from the refresh cookie. Concurrent callers share one request: refresh
 * tokens rotate and the server revokes the sign-in if an old one is replayed (docs/06 §3.3).
 */
let inflight: Promise<boolean> | null = null;
export function refresh(): Promise<boolean> {
  inflight ??= (async () => {
    try {
      const res = await fetch('/api/v1/auth/refresh', { method: 'POST', headers: CSRF, credentials: 'same-origin' });
      if (!res.ok) {
        accessToken = null;
        return false;
      }
      accessToken = (await res.json()).access_token;
      return true;
    } finally {
      inflight = null;
    }
  })();
  return inflight;
}

export async function api<T = unknown>(path: string, init: { method?: string; body?: unknown; headers?: Record<string, string>; raw?: string | Blob } = {}, retried = false): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json', ...init.headers };
  if (accessToken) headers.authorization = `Bearer ${accessToken}`;
  let body: BodyInit | undefined;
  if (init.raw !== undefined) {
    body = init.raw;
    if (init.raw instanceof Blob) headers['content-type'] ??= 'application/octet-stream';
  }
  else if (init.body !== undefined) {
    headers['content-type'] = 'application/json';
    body = JSON.stringify(init.body);
  }
  const res = await fetch(path, { method: init.method ?? 'GET', headers, body, credentials: 'same-origin' });
  if (res.status === 401 && !retried && !path.startsWith('/api/v1/auth/')) {
    const problem = await res.clone().json().catch(() => null);
    if (problem?.code !== 'step_up_required' && (await refresh())) return api<T>(path, init, true);
  }
  return parse(res) as Promise<T>;
}

/** Downloads a file response (e.g. a CSV export) with the bearer token and saves it as `filename`. */
export async function download(path: string, filename: string, retried = false): Promise<void> {
  const res = await fetch(path, { headers: accessToken ? { authorization: `Bearer ${accessToken}` } : {}, credentials: 'same-origin' });
  if (res.status === 401 && !retried && (await refresh())) return download(path, filename, true);
  if (!res.ok) await parse(res);
  const url = URL.createObjectURL(await res.blob());
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

/** Opens an authenticated HTML response (e.g. a print-ready export) in a new tab. */
export async function openHtml(path: string, retried = false): Promise<void> {
  const tab = window.open('', '_blank');
  const res = await fetch(path, { headers: accessToken ? { authorization: `Bearer ${accessToken}` } : {}, credentials: 'same-origin' });
  if (res.status === 401 && !retried && (await refresh())) { tab?.close(); return openHtml(path, true); }
  if (!res.ok) { tab?.close(); await parse(res); }
  const url = URL.createObjectURL(new Blob([await res.text()], { type: 'text/html' }));
  if (tab) tab.location.href = url; else window.location.href = url;
}

/** Same as `api` but for the cookie-bearing auth endpoints. */
export async function authPost<T = unknown>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'POST',
    headers: { ...CSRF, accept: 'application/json', ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    credentials: 'same-origin',
  });
  return parse(res) as Promise<T>;
}
