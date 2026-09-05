import type {
  FlagMatches,
  Session,
  SessionFilters,
  SessionPage,
  Source,
  SourceInput,
} from "./types";

const API_BASE = import.meta.env.VITE_API_BASE ?? "";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, init);
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.error ?? `Request failed with ${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export const api = {
  health: () => request<{ status: string }>("/api/health"),

  listSources: () => request<Source[]>("/api/sources"),

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
    const response = await fetch(
      `${API_BASE}/api/sessions/${id}/payload/${direction}`,
    );
    if (!response.ok) throw new Error(`Payload request failed with ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  },

  getFlagMatches: (id: number) =>
    request<FlagMatches>(`/api/sessions/${id}/flag-matches`),
};

