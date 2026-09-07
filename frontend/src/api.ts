import type {
  FlagMatches,
  Session,
  SessionFilters,
  SessionPage,
  Source,
  SourceInput,
  CollectorsResponse,
} from "./types";

export class ApiError extends Error {
  constructor(public status: number, message: string) { super(message); }
}

const unauthorizedListeners = new Set<() => void>();
let authGeneration = 0;

export function onUnauthorized(listener: () => void) {
  unauthorizedListeners.add(listener);
  return () => { unauthorizedListeners.delete(listener); };
}

async function fetchApi(path: string, init?: RequestInit, anonymous = false) {
  const generation = authGeneration;
  const response = await fetch(path, { ...init, credentials: "same-origin", cache: "no-store" });
  if (response.status === 401 && !anonymous && generation === authGeneration) {
    authGeneration++;
    unauthorizedListeners.forEach((listener) => listener());
  }
  return response;
}

async function request<T>(path: string, init?: RequestInit, anonymous = false): Promise<T> {
  const response = await fetchApi(path, init, anonymous);
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new ApiError(response.status, body?.error ?? `Request failed with ${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export const api = {
  me: () => request<{ authenticated: true; username: string }>("/api/auth/me", undefined, true),

  login: async (username: string, password: string) => {
    const identity = await request<{ authenticated: true; username: string }>("/api/auth/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ username, password }),
    }, true);
    authGeneration++;
    return identity;
  },

  logout: () => request<void>("/api/auth/logout", { method: "POST" }),

  health: () => request<{ status: string }>("/api/health"),

  listSources: () => request<Source[]>("/api/sources"),

  listCollectors: () => request<CollectorsResponse>("/api/collectors"),

  createSource: (input: SourceInput) =>
    request<Source>("/api/sources", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    }),

  updateSource: (id: number, input: SourceInput) =>
    request<Source>(`/api/sources/${id}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    }),

  deleteSource: (id: number) =>
    request<void>(`/api/sources/${id}`, { method: "DELETE" }),

  listSessions: (filters: SessionFilters = {}) => {
    const params = new URLSearchParams({ limit: "100" });
    if (filters.sourceId) params.set("source_id", String(filters.sourceId));
    if (filters.containsFlag) params.set("contains_flag", "true");
    if (filters.protocol) params.set("protocol", filters.protocol);
    if (filters.payload) params.set("payload", filters.payload);
    if (filters.cursor) params.set("cursor", String(filters.cursor));
    return request<SessionPage>(`/api/sessions?${params}`);
  },

  getSession: (id: number) => request<Session>(`/api/sessions/${id}`),

  getPayload: async (id: number, direction: "c2s" | "s2c") => {
    const response = await fetchApi(
      `/api/sessions/${id}/payload/${direction}`,
    );
    if (!response.ok) throw new Error(`Payload request failed with ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  },

  getFlagMatches: (id: number) =>
    request<FlagMatches>(`/api/sessions/${id}/flag-matches`),
};
