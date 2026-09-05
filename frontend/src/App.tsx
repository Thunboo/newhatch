import {
  AlertTriangle,
  ArrowDown,
  ArrowRight,
  ArrowUpDown,
  Braces,
  Check,
  Flag,
  ListFilter,
  Network,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Settings2,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { api } from "./api";
import type {
  ByteRange,
  FlagMatches,
  Protocol,
  Session,
  SessionFilters,
  Source,
  SourceInput,
} from "./types";

type View = "sessions" | "sources";
type PayloadMode = "text" | "hex";
type SourceSort = "name-asc" | "name-desc" | "port-asc" | "port-desc";

const emptySource: SourceInput = { name: "", port: 8080, enabled: true };

export function App() {
  const [view, setView] = useState<View>("sessions");
  const [online, setOnline] = useState(false);
  const [sources, setSources] = useState<Source[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [nextCursor, setNextCursor] = useState<number | null>(null);
  const [selected, setSelected] = useState<Session | null>(null);
  const [filters, setFilters] = useState<SessionFilters>({});
  const [searchValue, setSearchValue] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadSources = useCallback(async () => {
    try {
      setSources(await api.listSources());
      setOnline(true);
    } catch (caught) {
      setOnline(false);
      setError(messageOf(caught));
    }
  }, []);

  const loadSessions = useCallback(
    async (append = false) => {
      setLoading(true);
      setError(null);
      try {
        const page = await api.listSessions({
          ...filters,
          cursor: append && nextCursor ? nextCursor : undefined,
        });
        setSessions((current) => (append ? [...current, ...page.items] : page.items));
        setNextCursor(page.next_cursor);
        setOnline(true);
      } catch (caught) {
        setOnline(false);
        setError(messageOf(caught));
      } finally {
        setLoading(false);
      }
    },
    [filters, nextCursor],
  );

  useEffect(() => {
    void loadSources();
  }, [loadSources]);

  useEffect(() => {
    void loadSessions(false);
  }, [filters]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (view === "sessions" && !selected && !filters.payload) void loadSessions(false);
    }, 5_000);
    return () => window.clearInterval(timer);
  }, [filters.payload, loadSessions, selected, view]);

  function submitSearch(event: FormEvent) {
    event.preventDefault();
    setFilters((current) => ({
      ...current,
      payload: searchValue.trim() || undefined,
    }));
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><img src="/logo.png" alt="Нюхач" /></div>
        <nav aria-label="Primary navigation">
          <button className={view === "sessions" ? "nav-item active" : "nav-item"} onClick={() => setView("sessions")}>
            <Radio size={17} /> Sessions
          </button>
          <button className={view === "sources" ? "nav-item active" : "nav-item"} onClick={() => setView("sources")}>
            <Network size={17} /> Sources
          </button>
        </nav>
        <div className={online ? "connection online" : "connection offline"}>
          <span /> {online ? "Analyzer online" : "Analyzer offline"}
        </div>
      </aside>

      <main className="workspace">
        {view === "sessions" ? (
          <SessionsView
            sources={sources}
            sessions={sessions}
            selected={selected}
            filters={filters}
            searchValue={searchValue}
            loading={loading}
            error={error}
            nextCursor={nextCursor}
            onFilters={setFilters}
            onSearchValue={setSearchValue}
            onSearch={submitSearch}
            onRefresh={() => void loadSessions(false)}
            onLoadMore={() => void loadSessions(true)}
            onSelect={setSelected}
          />
        ) : (
          <SourcesView sources={sources} onChanged={loadSources} />
        )}
      </main>

      {selected && <SessionDetail session={selected} onClose={() => setSelected(null)} />}
    </div>
  );
}

type SessionsViewProps = {
  sources: Source[];
  sessions: Session[];
  selected: Session | null;
  filters: SessionFilters;
  searchValue: string;
  loading: boolean;
  error: string | null;
  nextCursor: number | null;
  onFilters: (updater: (current: SessionFilters) => SessionFilters) => void;
  onSearchValue: (value: string) => void;
  onSearch: (event: FormEvent) => void;
  onRefresh: () => void;
  onLoadMore: () => void;
  onSelect: (session: Session) => void;
};

function SessionsView(props: SessionsViewProps) {
  const protocols: Array<{ label: string; value?: Protocol }> = [
    { label: "All" },
    { label: "HTTP", value: "http" },
    { label: "WebSocket", value: "websocket" },
    { label: "Raw TCP", value: "raw_tcp" },
  ];

  return (
    <>
      <header className="page-header">
        <div><h1>Sessions</h1><p>{props.sessions.length} recent connections</p></div>
        <button className="icon-button" title="Refresh sessions" onClick={props.onRefresh} disabled={props.loading}>
          <RefreshCw size={17} className={props.loading ? "spin" : ""} />
        </button>
      </header>

      <section className="filter-bar">
        <form className="search-box" onSubmit={props.onSearch}>
          <Search size={16} />
          <input value={props.searchValue} onChange={(event) => props.onSearchValue(event.target.value)} placeholder="Search reconstructed payload" />
          {props.searchValue && <button type="button" title="Clear search" onClick={() => props.onSearchValue("")}><X size={15} /></button>}
        </form>
        <select
          aria-label="Source filter"
          value={props.filters.sourceId ?? ""}
          onChange={(event) => props.onFilters((current) => ({ ...current, sourceId: Number(event.target.value) || undefined }))}
        >
          <option value="">All sources</option>
          {props.sources.map((source) => <option key={source.id} value={source.id}>{source.name} :{source.port}</option>)}
        </select>
        <label className="flag-toggle">
          <input
            type="checkbox"
            checked={props.filters.containsFlag ?? false}
            onChange={(event) => props.onFilters((current) => ({ ...current, containsFlag: event.target.checked || undefined }))}
          />
          <Flag size={15} /> Flags only
        </label>
      </section>

      <div className="protocol-tabs" role="tablist" aria-label="Protocol filter">
        {protocols.map((protocol) => (
          <button
            key={protocol.label}
            className={props.filters.protocol === protocol.value ? "active" : ""}
            onClick={() => props.onFilters((current) => ({ ...current, protocol: protocol.value }))}
          >{protocol.label}</button>
        ))}
      </div>

      {props.error && <div className="error-strip"><AlertTriangle size={16} /> {props.error}</div>}

      <div className="table-wrap">
        <table className="session-table">
          <thead><tr><th>Time</th><th>Source</th><th>Client</th><th></th><th>Server</th><th>Protocol</th><th>Size</th><th>Flag</th></tr></thead>
          <tbody>
            {props.sessions.map((session) => (
              <tr key={session.id} className={session.contains_flag ? "flag-row" : ""} onClick={() => props.onSelect(session)}>
                <td className="mono time-cell">{formatTime(session.started_at)}</td>
                <td><span className="source-name">{session.source_name}</span></td>
                <td className="mono endpoint">{formatEndpoint(session.client_ip, session.client_port)}</td>
                <td className="arrow-cell"><ArrowRight size={14} /></td>
                <td className="mono endpoint">{formatEndpoint(session.server_ip, session.server_port)}</td>
                <td><span className={`protocol protocol-${session.protocol}`}>{protocolLabel(session.protocol)}</span></td>
                <td className="mono">{formatBytes(session.bytes_c2s + session.bytes_s2c)}</td>
                <td>
                  <div className="signals">
                    {session.contains_flag && <span className="flag-signal" title={`${session.flag_count} flag matches`}><Flag size={14} /> {session.flag_count}</span>}
                    {session.suricata_alerts > 0 && <span className="alert-signal" title="Suricata alerts"><AlertTriangle size={14} /> {session.suricata_alerts}</span>}
                    {session.incomplete && <span className="incomplete-dot" title="Incomplete session" />}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!props.loading && props.sessions.length === 0 && (
          <div className="empty-state"><ListFilter size={20} /><strong>No sessions match</strong></div>
        )}
      </div>
      {props.nextCursor && <button className="load-more" onClick={props.onLoadMore} disabled={props.loading}><ArrowDown size={15} /> Load older</button>}
    </>
  );
}

function SourcesView({ sources, onChanged }: { sources: Source[]; onChanged: () => Promise<void> }) {
  const [draft, setDraft] = useState<SourceInput>(emptySource);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SourceSort>("name-asc");

  const visibleSources = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    const filtered = sources.filter((source) => (
      source.name.toLocaleLowerCase().includes(normalizedQuery)
      || String(source.port).includes(normalizedQuery)
    ));

    return filtered.sort((left, right) => {
      const direction = sort.endsWith("desc") ? -1 : 1;
      const comparison = sort.startsWith("name")
        ? left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: "base" })
        : left.port - right.port;
      return direction * (comparison || left.id - right.id);
    });
  }, [query, sort, sources]);

  async function create(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await api.createSource(draft);
      setDraft(emptySource);
      await onChanged();
    } catch (caught) {
      setError(messageOf(caught));
    } finally {
      setSaving(false);
    }
  }

  async function toggle(source: Source) {
    try {
      await api.updateSource(source.id, { name: source.name, port: source.port, enabled: !source.enabled });
      await onChanged();
    } catch (caught) { setError(messageOf(caught)); }
  }

  async function remove(source: Source) {
    if (!window.confirm(`Delete source "${source.name}"?`)) return;
    try {
      await api.deleteSource(source.id);
      await onChanged();
    } catch (caught) { setError(messageOf(caught)); }
  }

  return (
    <>
      <header className="page-header"><div><h1>Sources</h1><p>Kernel capture filter inputs</p></div></header>
      <form className="source-form" onSubmit={create}>
        <label>Name<input required maxLength={80} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} placeholder="web" /></label>
        <label>TCP port<input required type="number" min={1} max={65535} value={draft.port} onChange={(event) => setDraft({ ...draft, port: Number(event.target.value) })} /></label>
        <label className="enabled-field"><input type="checkbox" checked={draft.enabled} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} /> Enabled</label>
        <button className="primary-button" disabled={saving}><Plus size={16} /> Add source</button>
      </form>
      {error && <div className="error-strip"><AlertTriangle size={16} /> {error}</div>}
      <section className="source-tools" aria-label="Source list controls">
        <div className="search-box">
          <Search size={16} />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search by name or port"
            aria-label="Search sources by name or port"
          />
          {query && <button type="button" title="Clear search" onClick={() => setQuery("")}><X size={15} /></button>}
        </div>
        <label className="source-sort">
          <ArrowUpDown size={15} />
          <select value={sort} onChange={(event) => setSort(event.target.value as SourceSort)} aria-label="Sort sources">
            <option value="name-asc">Name A-Z</option>
            <option value="name-desc">Name Z-A</option>
            <option value="port-asc">Port low-high</option>
            <option value="port-desc">Port high-low</option>
          </select>
        </label>
      </section>
      <div className="source-list">
        <div className="source-list-head"><span>Name</span><span>Port</span><span>Status</span><span></span></div>
        {visibleSources.map((source) => (
          <div className="source-row" key={source.id}>
            <span className="source-title"><Network size={16} /> {source.name}</span>
            <span className="mono">{source.port}</span>
            <button className={source.enabled ? "status-toggle enabled" : "status-toggle"} onClick={() => void toggle(source)}>
              <span>{source.enabled ? <Check size={12} /> : null}</span>{source.enabled ? "Enabled" : "Disabled"}
            </button>
            <button className="icon-button danger" title="Delete source" onClick={() => void remove(source)}><Trash2 size={16} /></button>
          </div>
        ))}
        {visibleSources.length === 0 && (
          <div className="empty-state">
            {sources.length === 0 ? <Network size={20} /> : <ListFilter size={20} />}
            <strong>{sources.length === 0 ? "No monitored sources" : "No sources match"}</strong>
          </div>
        )}
      </div>
    </>
  );
}

function SessionDetail({ session, onClose }: { session: Session; onClose: () => void }) {
  const [payloads, setPayloads] = useState<{ c2s: Uint8Array; s2c: Uint8Array } | null>(null);
  const [matches, setMatches] = useState<FlagMatches>({ c2s: [], s2c: [] });
  const [mode, setMode] = useState<PayloadMode>("text");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([api.getPayload(session.id, "c2s"), api.getPayload(session.id, "s2c"), api.getFlagMatches(session.id)])
      .then(([c2s, s2c, found]) => { if (active) { setPayloads({ c2s, s2c }); setMatches(found); } })
      .catch((caught) => { if (active) setError(messageOf(caught)); });
    return () => { active = false; };
  }, [session.id]);

  return (
    <aside className="detail-panel" aria-label="Session detail">
      <header className="detail-header">
        <div><span className="eyebrow">Session #{session.id}</span><h2>{session.http.method ? `${session.http.method} ${session.http.path ?? ""}` : protocolLabel(session.protocol)}</h2></div>
        <button className="icon-button" title="Close session" onClick={onClose}><X size={18} /></button>
      </header>
      <div className="detail-meta">
        <span><small>Source</small>{session.source_name} :{session.server_port}</span>
        <span><small>Started</small>{formatDateTime(session.started_at)}</span>
        <span><small>Traffic</small>{formatBytes(session.bytes_c2s + session.bytes_s2c)}</span>
        <span><small>Protocol</small>{protocolLabel(session.protocol)}</span>
      </div>
      {session.contains_flag && <div className="flag-banner"><Flag size={16} /><strong>{session.flag_count} flag match{session.flag_count === 1 ? "" : "es"}</strong><span>{session.flag_direction.toUpperCase()}</span></div>}
      {error && <div className="error-strip"><AlertTriangle size={16} /> {error}</div>}
      <div className="payload-toolbar">
        <div className="segmented" role="group" aria-label="Payload format">
          <button className={mode === "text" ? "active" : ""} onClick={() => setMode("text")}><Braces size={14} /> Text</button>
          <button className={mode === "hex" ? "active" : ""} onClick={() => setMode("hex")}><Settings2 size={14} /> Hex</button>
        </div>
      </div>
      {!payloads ? <div className="payload-loading"><RefreshCw className="spin" size={17} /></div> : (
        <div className="streams">
          <Stream title="Client -> server" bytes={payloads.c2s} ranges={matches.c2s} mode={mode} flagged={session.flag_direction === "c2s" || session.flag_direction === "both"} />
          <Stream title="Server -> client" bytes={payloads.s2c} ranges={matches.s2c} mode={mode} flagged={session.flag_direction === "s2c" || session.flag_direction === "both"} />
        </div>
      )}
    </aside>
  );
}

function Stream({ title, bytes, ranges, mode, flagged }: { title: string; bytes: Uint8Array; ranges: ByteRange[]; mode: PayloadMode; flagged: boolean }) {
  return (
    <section className={flagged ? "stream flagged" : "stream"}>
      <header><span>{title}</span><span className="mono">{formatBytes(bytes.length)}</span></header>
      <pre>{mode === "hex" ? toHex(bytes) : <HighlightedBytes bytes={bytes} ranges={ranges} />}</pre>
    </section>
  );
}

function HighlightedBytes({ bytes, ranges }: { bytes: Uint8Array; ranges: ByteRange[] }) {
  const decoder = useMemo(() => new TextDecoder("utf-8", { fatal: false }), []);
  if (ranges.length === 0) return <>{decoder.decode(bytes)}</>;
  const chunks: ReactNode[] = [];
  let offset = 0;
  for (const range of ranges) {
    if (range.start > offset) chunks.push(decoder.decode(bytes.slice(offset, range.start)));
    chunks.push(<mark key={`${range.start}-${range.end}`}>{decoder.decode(bytes.slice(range.start, range.end))}</mark>);
    offset = range.end;
  }
  if (offset < bytes.length) chunks.push(decoder.decode(bytes.slice(offset)));
  return <>{chunks}</>;
}

function toHex(bytes: Uint8Array): string {
  const lines: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const slice = bytes.slice(offset, offset + 16);
    const hex = Array.from(slice, (byte) => byte.toString(16).padStart(2, "0")).join(" ").padEnd(47, " ");
    const ascii = Array.from(slice, (byte) => byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : ".").join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  ${ascii}`);
  }
  return lines.join("\n");
}

function protocolLabel(protocol: Protocol): string {
  return protocol === "raw_tcp" ? "Raw TCP" : protocol === "websocket" ? "WebSocket" : "HTTP";
}

function formatEndpoint(ip: string, port: number): string {
  return ip.includes(":") ? `[${ip}]:${port}` : `${ip}:${port}`;
}

function formatTime(micros: number): string {
  return new Date(micros / 1_000).toLocaleTimeString([], { hour12: false });
}

function formatDateTime(micros: number): string {
  return new Date(micros / 1_000).toLocaleString([], { hour12: false });
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : "Unexpected error";
}
