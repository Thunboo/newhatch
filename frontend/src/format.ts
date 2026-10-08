import type { Protocol } from "./types";

export function protocolLabel(protocol: Protocol, rawTcpLabel: string): string {
  return protocol === "raw_tcp" ? rawTcpLabel : protocol === "websocket" ? "WebSocket" : "HTTP";
}

export function formatEndpoint(ip: string, port: number): string {
  return ip.includes(":") ? `[${ip}]:${port}` : `${ip}:${port}`;
}

export function formatTime(micros: number, locale: string): string {
  return new Date(micros / 1_000).toLocaleTimeString(locale, { hour12: false });
}

export function formatDateTime(micros: number, locale: string): string {
  return new Date(micros / 1_000).toLocaleString(locale, { hour12: false });
}

export function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

export function messageOf(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}
