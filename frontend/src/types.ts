export type Source = {
  id: number;
  name: string;
  port: number;
  enabled: boolean;
};

export type SourceInput = Omit<Source, "id">;

export type Collector = {
  collector_id: string;
  connected: boolean;
  peer: string;
  connected_at: number;
  last_activity: number;
  captured_packets: number;
  sent_packets: number;
  dropped_packets: number;
  queue_depth: number;
  reconnect_count: number;
  receiver_dropped_packets: number;
  last_error: string | null;
};

export type CollectorsResponse = {
  mode: "local" | "remote";
  collectors: Collector[];
};

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
  collector_id: string | null;
  first_payload_c2s_at: number | null;
  first_payload_s2c_at: number | null;
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

export type Chain = {
  id: number;
  collector_id: string | null;
  source_id: number;
  source_name: string;
  client_ip: string;
  started_at: number;
  ended_at: number;
  session_count: number;
  bytes_c2s: number;
  bytes_s2c: number;
  contains_flag: boolean;
  flag_count: number;
  suricata_alerts: number;
  incomplete: boolean;
  snapshot_id: number;
};
export type ChainPage = { items: Chain[]; next_cursor: string | null };
export type ChainMemberPage = { chain: Chain; items: Session[]; next_cursor: string | null };
