export type Source = {
  id: number;
  name: string;
  port: number;
  enabled: boolean;
};

export type SourceInput = Omit<Source, "id">;

export type Protocol = "raw_tcp" | "http" | "websocket";
export type FlagDirection = "none" | "c2s" | "s2c" | "both";

export type HttpMetadata = {
  method: string | null;
  host: string | null;
  path: string | null;
  status: number | null;
  content_type: string | null;
};

export type Session = {
  id: number;
  source_id: number;
  source_name: string;
  started_at: number;
  ended_at: number;
  client_ip: string;
  client_port: number;
  server_ip: string;
  server_port: number;
  protocol: Protocol;
  bytes_c2s: number;
  bytes_s2c: number;
  contains_flag: boolean;
  flag_direction: FlagDirection;
  flag_count: number;
  suricata_alerts: number;
  incomplete: boolean;
  http: HttpMetadata;
};

export type SessionPage = {
  items: Session[];
  next_cursor: number | null;
};

export type ByteRange = { start: number; end: number };
export type FlagMatches = { c2s: ByteRange[]; s2c: ByteRange[] };

export type SessionFilters = {
  sourceId?: number;
  containsFlag?: boolean;
  protocol?: Protocol;
  payload?: string;
  cursor?: number;
};

