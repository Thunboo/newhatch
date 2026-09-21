import {
  AlertTriangle,
  ArrowRight,
  ArrowUpDown,
  Braces,
  Check,
  Copy,
  Flag,
  ListFilter,
  Languages,
  LogOut,
  Network,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server,
  Settings2,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { api } from "./api";
import { AuthGate } from "./AuthGate";
import logoUrl from "./assets/logo.png";
import { useI18n } from "./i18n";
import { decodeDisplayEscapes, decodePayload, displayPayload } from "./payloadDisplay";
import type {
  ByteRange,
  FlagMatches,
  Protocol,
  Session,
  SessionFilters,
  Source,
  SourceInput,
  Collector,
} from "./types";

type View = "sessions" | "sources" | "collectors";
type PayloadMode = "text" | "hex";
type SourceSort = "name-asc" | "name-desc" | "port-asc" | "port-desc";

const emptySource: SourceInput = { name: "", port: 8080, enabled: true };
const LIVE_REFRESH_INTERVAL_MS = 5_000;
const LIVE_EDGE_THRESHOLD_PX = 80;

type ScrollAnchor = { sessionId: number; top: number };

function mergeSessions(current: Session[], incoming: Session[]) {
  const byId = new Map(current.map((session) => [session.id, session]));
  incoming.forEach((session) => byId.set(session.id, session));
  return Array.from(byId.values()).sort((left, right) => right.id - left.id);
}

function captureSessionAnchor(): ScrollAnchor | null {
  const rows = document.querySelectorAll<HTMLElement>("[data-session-id]");
  for (const row of rows) {
    const bounds = row.getBoundingClientRect();
    if (bounds.bottom > 0 && bounds.top < window.innerHeight) {
      const sessionId = Number(row.dataset.sessionId);
      if (Number.isSafeInteger(sessionId)) return { sessionId, top: bounds.top };
    }
  }
  return null;
}

export function App() {
  return <AuthGate>{(logout) => <AuthenticatedApp onLogout={logout} />}</AuthGate>;
}

function AuthenticatedApp({ onLogout }: { onLogout: () => Promise<void> }) {
  const { t, toggleLanguage } = useI18n();
  const [loggingOut, setLoggingOut] = useState(false);
  const [view, setView] = useState<View>("sessions");
  const [online, setOnline] = useState(false);
  const [sources, setSources] = useState<Source[]>([]);
  const [collectors, setCollectors] = useState<Collector[]>([]);
  const [analyzerMode, setAnalyzerMode] = useState<"local" | "remote">("local");
  const [sessions, setSessions] = useState<Session[]>([]);
  const [olderCursor, setOlderCursorState] = useState<number | null>(null);
  const [selected, setSelected] = useState<Session | null>(null);
  const [filters, setFilters] = useState<SessionFilters>({});
  const [searchValue, setSearchValue] = useState("");
  const [initialLoading, setInitialLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [olderError, setOlderError] = useState<string | null>(null);
  const [atLiveEdge, setAtLiveEdge] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const sessionsRef = useRef<Session[]>([]);
  const olderCursorRef = useRef<number | null>(null);
  const feedGenerationRef = useRef(0);
  const initialGenerationRef = useRef<number | null>(null);
  const refreshGenerationRef = useRef<number | null>(null);
  const olderRequestRef = useRef<{ generation: number; cursor: number } | null>(null);
  const liveEdgeRef = useRef(true);
  const pendingAnchorRef = useRef<ScrollAnchor | null>(null);

  const updateOlderCursor = useCallback((cursor: number | null) => {
    olderCursorRef.current = cursor;
    setOlderCursorState(cursor);
  }, []);

  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  useLayoutEffect(() => {
    const anchor = pendingAnchorRef.current;
    if (!anchor) return;
    pendingAnchorRef.current = null;
    const row = document.querySelector<HTMLElement>(`[data-session-id="${anchor.sessionId}"]`);
    if (!row) return;
    window.scrollBy({ top: row.getBoundingClientRect().top - anchor.top, left: 0 });
  }, [sessions]);

  const loadSources = useCallback(async () => {
    try {
      setSources(await api.listSources());
      setOnline(true);
    } catch (caught) {
      setOnline(false);
      setError(messageOf(caught, t("error.unexpected")));
    }
  }, [t]);

  const refreshSessions = useCallback(async () => {
    const generation = feedGenerationRef.current;
    if (initialGenerationRef.current === generation || refreshGenerationRef.current === generation) return;
    refreshGenerationRef.current = generation;
    setRefreshing(true);
    setError(null);
    try {
      const page = await api.listSessions(filters);
      if (generation !== feedGenerationRef.current) return;
      if (!liveEdgeRef.current) pendingAnchorRef.current = captureSessionAnchor();
      setSessions((current) => mergeSessions(current, page.items));
      setOnline(true);
    } catch (caught) {
      if (generation !== feedGenerationRef.current) return;
      setOnline(false);
      setError(messageOf(caught, t("error.unexpected")));
    } finally {
      if (refreshGenerationRef.current === generation) {
        refreshGenerationRef.current = null;
        setRefreshing(false);
      }
    }
  }, [filters, t]);

  const loadOlderSessions = useCallback(async () => {
    const generation = feedGenerationRef.current;
    const cursor = olderCursorRef.current;
    if (cursor === null || olderRequestRef.current) return;
    olderRequestRef.current = { generation, cursor };
    setLoadingOlder(true);
    setOlderError(null);
    try {
      const page = await api.listSessions({ ...filters, cursor });
      if (generation !== feedGenerationRef.current || olderCursorRef.current !== cursor) return;
      const loadedIds = new Set(sessionsRef.current.map((session) => session.id));
      const added = page.items.filter((session) => !loadedIds.has(session.id)).length;
      if (page.next_cursor === cursor || (page.items.length > 0 && added === 0)) {
        setOlderError(t("history.noProgress"));
        return;
      }
      setSessions((current) => mergeSessions(current, page.items));
      updateOlderCursor(page.next_cursor);
      setOnline(true);
    } catch (caught) {
      if (generation !== feedGenerationRef.current) return;
      setOnline(false);
      setOlderError(messageOf(caught, t("error.unexpected")));
    } finally {
      const request = olderRequestRef.current;
      if (request?.generation === generation && request.cursor === cursor) {
        olderRequestRef.current = null;
        setLoadingOlder(false);
      }
    }
  }, [filters, t, updateOlderCursor]);

  const loadCollectors = useCallback(async () => {
    try {
      const response = await api.listCollectors();
      setAnalyzerMode(response.mode);
      setCollectors(response.collectors);
      setOnline(true);
    } catch (caught) {
      setOnline(false);
      setError(messageOf(caught, t("error.unexpected")));
    }
  }, [t]);

  useEffect(() => {
    void loadSources();
    void loadCollectors();
  }, [loadCollectors, loadSources]);

  useEffect(() => {
    const generation = ++feedGenerationRef.current;
    initialGenerationRef.current = generation;
    refreshGenerationRef.current = null;
    olderRequestRef.current = null;
    pendingAnchorRef.current = null;
    sessionsRef.current = [];
    setSessions([]);
    updateOlderCursor(null);
    setInitialLoading(true);
    setRefreshing(false);
    setLoadingOlder(false);
    setOlderError(null);
    setError(null);
    liveEdgeRef.current = true;
    setAtLiveEdge(true);
    window.scrollTo({ top: 0, left: 0 });

    void api.listSessions(filters).then((page) => {
      if (generation !== feedGenerationRef.current) return;
      const unique = mergeSessions([], page.items);
      sessionsRef.current = unique;
      setSessions(unique);
      updateOlderCursor(page.next_cursor);
      setOnline(true);
    }).catch((caught) => {
      if (generation !== feedGenerationRef.current) return;
      setOnline(false);
      setError(messageOf(caught, t("error.unexpected")));
    }).finally(() => {
      if (initialGenerationRef.current === generation) {
        initialGenerationRef.current = null;
        setInitialLoading(false);
      }
    });
  }, [filters, t, updateOlderCursor]);

  useEffect(() => {
    let frame = 0;
    let lastPosition = window.scrollY;
    let direction: "up" | "down" | null = null;
    const update = () => {
      frame = 0;
      const position = window.scrollY;
      const nextDirection = position < lastPosition ? "up" : position > lastPosition ? "down" : direction;
      const nextLiveEdge = position <= LIVE_EDGE_THRESHOLD_PX;
      if (nextLiveEdge !== liveEdgeRef.current) {
        liveEdgeRef.current = nextLiveEdge;
        setAtLiveEdge(nextLiveEdge);
      }
      if (
        nextDirection === "up"
        && direction !== "up"
        && !nextLiveEdge
        && view === "sessions"
        && !selected
        && !filters.payload
      ) {
        void refreshSessions();
      }
      direction = nextDirection;
      lastPosition = position;
    };
    const onScroll = () => {
      if (!frame) frame = window.requestAnimationFrame(update);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    update();
    return () => {
      window.removeEventListener("scroll", onScroll);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [filters.payload, refreshSessions, selected, view]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (view === "sessions" && !selected && !filters.payload && atLiveEdge) void refreshSessions();
      if (view === "collectors" || analyzerMode === "remote") void loadCollectors();
    }, LIVE_REFRESH_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [analyzerMode, atLiveEdge, filters.payload, loadCollectors, refreshSessions, selected, view]);

  useEffect(() => {
    if (view === "collectors") void loadCollectors();
  }, [loadCollectors, view]);

  function submitSearch(event: FormEvent) {
    event.preventDefault();
    setFilters((current) => ({
      ...current,
      payload: searchValue.trim() || undefined,
    }));
  }

  const collectorOnline = collectors.some((collector) => collector.connected);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><img src={logoUrl} alt="Нюхач" /></div>
        <nav aria-label={t("nav.primary")}>
          <button title={t("nav.sessions")} className={view === "sessions" ? "nav-item active" : "nav-item"} onClick={() => setView("sessions")}>
            <Radio size={17} /> {t("nav.sessions")}
          </button>
          <button title={t("nav.sources")} className={view === "sources" ? "nav-item active" : "nav-item"} onClick={() => setView("sources")}>
            <Network size={17} /> {t("nav.sources")}
          </button>
          <button title={t("nav.collectors")} className={view === "collectors" ? "nav-item active" : "nav-item"} onClick={() => setView("collectors")}>
            <Server size={17} /> {t("nav.collectors")}
          </button>
        </nav>
        <div className="connection-statuses">
          <div className={online ? "connection online" : "connection offline"} title={online ? t("status.analyzerOnline") : t("status.analyzerOffline")}>
            <span /> {online ? t("status.analyzerOnline") : t("status.analyzerOffline")}
          </div>
          {analyzerMode === "remote" && (
            <div className={collectorOnline ? "connection online" : "connection offline"} title={collectorOnline ? t("status.collectorOnline") : t("status.collectorOffline")}>
              <span /> {collectorOnline ? t("status.collectorOnline") : t("status.collectorOffline")}
            </div>
          )}
        </div>
        <button className="nav-item language-button" title={t("language.action")} aria-label={t("language.action")} onClick={toggleLanguage}>
          <Languages size={17} /><span>{t("language.target")}</span>
        </button>
        <button className="nav-item logout-button" title={t("auth.signOut")} aria-label={t("auth.signOut")} disabled={loggingOut} onClick={() => {
          setLoggingOut(true);
          void onLogout().catch(() => setError(t("auth.signOutFailed"))).finally(() => setLoggingOut(false));
        }}><LogOut size={17} /><span>{t("auth.signOut")}</span></button>
      </aside>

      <main className="workspace">
        {view === "sessions" ? (
          <SessionsView
            sources={sources}
            sessions={sessions}
            selected={selected}
            filters={filters}
            searchValue={searchValue}
            initialLoading={initialLoading}
            refreshing={refreshing}
            loadingOlder={loadingOlder}
            olderError={olderError}
            error={error}
            hasOlder={olderCursor !== null}
            onFilters={setFilters}
            onSearchValue={setSearchValue}
            onSearch={submitSearch}
            onRefresh={() => void refreshSessions()}
            onLoadOlder={() => void loadOlderSessions()}
            onSelect={setSelected}
          />
        ) : view === "sources" ? (
          <SourcesView sources={sources} onChanged={loadSources} />
        ) : (
          <CollectorsView mode={analyzerMode} collectors={collectors} onRefresh={() => void loadCollectors()} />
        )}
      </main>

      {selected && <SessionDetail session={selected} onClose={() => setSelected(null)} />}
    </div>
  );
}

function CollectorsView({ mode, collectors, onRefresh }: { mode: "local" | "remote"; collectors: Collector[]; onRefresh: () => void }) {
  const { locale, t } = useI18n();
  return (
    <>
      <header className="page-header">
        <div><h1>{t("nav.collectors")}</h1><p>{mode === "local" ? t("collectors.localActive") : t("collectors.connected", { count: collectors.filter((item) => item.connected).length })}</p></div>
        <button className="icon-button" title={t("collectors.refresh")} aria-label={t("collectors.refresh")} onClick={onRefresh}><RefreshCw size={17} /></button>
      </header>
      {mode === "local" && (
        <div className="local-collector-notice">
          <Server size={20} />
          <div>
            <strong>{t("collectors.localTitle")}</strong>
            <p>{t("collectors.localDescriptionBefore")}<code>ANALYZER=remote</code>{t("collectors.localDescriptionAfter")}</p>
          </div>
        </div>
      )}
      {mode === "remote" && (
      <div className="collector-list">
        {collectors.map((collector) => (
          <article className="collector-row" key={collector.collector_id}>
            <div className="collector-identity">
              <span className={collector.connected ? "collector-state online" : "collector-state"} />
              <div><strong>{collector.collector_id}</strong><span className="mono">{collector.peer}</span></div>
            </div>
            <CollectorMetric label={t("collectors.captured")} value={collector.captured_packets} />
            <CollectorMetric label={t("collectors.sent")} value={collector.sent_packets} />
            <CollectorMetric label={t("collectors.dropped")} value={collector.dropped_packets + collector.receiver_dropped_packets} warning={collector.dropped_packets + collector.receiver_dropped_packets > 0} />
            <CollectorMetric label={t("collectors.queue")} value={collector.queue_depth} />
            <CollectorMetric label={t("collectors.reconnects")} value={collector.reconnect_count} />
            <div className="collector-activity"><small>{t("collectors.lastActivity")}</small><span>{formatDateTime(collector.last_activity, locale)}</span></div>
            {collector.last_error && <div className="collector-error" title={collector.last_error}><AlertTriangle size={14} /> {collector.last_error}</div>}
          </article>
        ))}
        {collectors.length === 0 && <div className="empty-state"><Server size={20} /><strong>{t("collectors.empty")}</strong></div>}
      </div>
      )}
    </>
  );
}

function CollectorMetric({ label, value, warning = false }: { label: string; value: number; warning?: boolean }) {
  const { locale } = useI18n();
  return <div className={warning ? "collector-metric warning" : "collector-metric"}><small>{label}</small><strong className="mono">{value.toLocaleString(locale)}</strong></div>;
}

type SessionsViewProps = {
  sources: Source[];
  sessions: Session[];
  selected: Session | null;
  filters: SessionFilters;
  searchValue: string;
  initialLoading: boolean;
  refreshing: boolean;
  loadingOlder: boolean;
  olderError: string | null;
  error: string | null;
  hasOlder: boolean;
  onFilters: (updater: (current: SessionFilters) => SessionFilters) => void;
  onSearchValue: (value: string) => void;
  onSearch: (event: FormEvent) => void;
  onRefresh: () => void;
  onLoadOlder: () => void;
  onSelect: (session: Session) => void;
};

function SessionsView(props: SessionsViewProps) {
  const { locale, t } = useI18n();
  const historySentinel = useRef<HTMLDivElement>(null);
  const protocols: Array<{ label: string; value?: Protocol }> = [
    { label: t("sessions.all") },
    { label: "HTTP", value: "http" },
    { label: "WebSocket", value: "websocket" },
    { label: t("sessions.rawTcp"), value: "raw_tcp" },
  ];

  useEffect(() => {
    const sentinel = historySentinel.current;
    if (!sentinel || !props.hasOlder || props.loadingOlder || props.olderError) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) props.onLoadOlder();
    }, { rootMargin: "0px 0px 480px" });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [props.hasOlder, props.loadingOlder, props.olderError, props.onLoadOlder, props.sessions.length]);

  return (
    <>
      <header className="page-header">
        <div><h1>{t("nav.sessions")}</h1><p>{t("sessions.loaded", { count: props.sessions.length })}</p></div>
        <button className="icon-button" title={t("sessions.refresh")} aria-label={t("sessions.refresh")} onClick={props.onRefresh} disabled={props.initialLoading || props.refreshing}>
          <RefreshCw size={17} className={props.initialLoading || props.refreshing ? "spin" : ""} />
        </button>
      </header>

      <section className="filter-bar">
        <form className="search-box" onSubmit={props.onSearch}>
          <Search size={16} />
          <input value={props.searchValue} onChange={(event) => props.onSearchValue(event.target.value)} placeholder={t("sessions.searchPayload")} />
          {props.searchValue && <button type="button" title={t("common.clearSearch")} aria-label={t("common.clearSearch")} onClick={() => props.onSearchValue("")}><X size={15} /></button>}
        </form>
        <select
          aria-label={t("sessions.sourceFilter")}
          value={props.filters.sourceId ?? ""}
          onChange={(event) => props.onFilters((current) => ({ ...current, sourceId: Number(event.target.value) || undefined }))}
        >
          <option value="">{t("sessions.allSources")}</option>
          {props.sources.map((source) => <option key={source.id} value={source.id}>{source.name} :{source.port}</option>)}
        </select>
        <label className="flag-toggle">
          <input
            type="checkbox"
            checked={props.filters.containsFlag ?? false}
            onChange={(event) => props.onFilters((current) => ({ ...current, containsFlag: event.target.checked || undefined }))}
          />
          <Flag size={15} /> {t("sessions.flagsOnly")}
        </label>
      </section>

      <div className="protocol-tabs" role="tablist" aria-label={t("sessions.protocolFilter")}>
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
          <thead><tr><th>{t("sessions.time")}</th><th>{t("common.source")}</th><th>{t("sessions.client")}</th><th></th><th>{t("sessions.server")}</th><th>{t("common.protocol")}</th><th>{t("sessions.size")}</th><th>{t("sessions.flag")}</th></tr></thead>
          <tbody>
            {props.sessions.map((session) => (
              <tr
                key={session.id}
                data-session-id={session.id}
                className={[session.contains_flag ? "flag-row" : "", props.selected?.id === session.id ? "selected-row" : ""].filter(Boolean).join(" ")}
                onClick={() => props.onSelect(session)}
              >
                <td className="mono time-cell">{formatTime(session.started_at, locale)}</td>
                <td><span className="source-name">{session.source_name}</span></td>
                <td className="mono endpoint">{formatEndpoint(session.client_ip, session.client_port)}</td>
                <td className="arrow-cell"><ArrowRight size={14} /></td>
                <td className="mono endpoint">{formatEndpoint(session.server_ip, session.server_port)}</td>
                <td><span className={`protocol protocol-${session.protocol}`}>{protocolLabel(session.protocol, t("sessions.rawTcp"))}</span></td>
                <td className="mono">{formatBytes(session.bytes_c2s + session.bytes_s2c)}</td>
                <td>
                  <div className="signals">
                    {session.contains_flag && <span className="flag-signal" title={t("sessions.flagMatches", { count: session.flag_count })}><Flag size={14} /> {session.flag_count}</span>}
                    {session.suricata_alerts > 0 && <span className="alert-signal" title={t("sessions.suricataAlerts")}><AlertTriangle size={14} /> {session.suricata_alerts}</span>}
                    {session.incomplete && <span className="incomplete-dot" title={t("sessions.incomplete")} />}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!props.initialLoading && props.sessions.length === 0 && (
          <div className="empty-state"><ListFilter size={20} /><strong>{t("sessions.empty")}</strong></div>
        )}
      </div>
      <div ref={historySentinel} className="history-sentinel" role="status" aria-live="polite">
        {props.loadingOlder ? t("sessions.loadingOlder") : props.olderError ? (
          <><span>{props.olderError}</span><button onClick={props.onLoadOlder}>{t("common.retry")}</button></>
        ) : !props.hasOlder && props.sessions.length > 0 ? t("sessions.allShown") : null}
      </div>
    </>
  );
}

function SourcesView({ sources, onChanged }: { sources: Source[]; onChanged: () => Promise<void> }) {
  const { locale, t } = useI18n();
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
        ? left.name.localeCompare(right.name, locale, { numeric: true, sensitivity: "base" })
        : left.port - right.port;
      return direction * (comparison || left.id - right.id);
    });
  }, [locale, query, sort, sources]);

  async function create(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await api.createSource(draft);
      setDraft(emptySource);
      await onChanged();
    } catch (caught) {
      setError(messageOf(caught, t("error.unexpected")));
    } finally {
      setSaving(false);
    }
  }

  async function toggle(source: Source) {
    try {
      await api.updateSource(source.id, { name: source.name, port: source.port, enabled: !source.enabled });
      await onChanged();
    } catch (caught) { setError(messageOf(caught, t("error.unexpected"))); }
  }

  async function remove(source: Source) {
    if (!window.confirm(t("sources.deleteConfirm", { name: source.name }))) return;
    try {
      await api.deleteSource(source.id);
      await onChanged();
    } catch (caught) { setError(messageOf(caught, t("error.unexpected"))); }
  }

  return (
    <>
      <header className="page-header"><div><h1>{t("nav.sources")}</h1><p>{t("sources.subtitle")}</p></div></header>
      <form className="source-form" onSubmit={create}>
        <label>{t("sources.name")}<input required maxLength={80} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} placeholder="web" /></label>
        <label>{t("sources.tcpPort")}<input required type="number" min={1} max={65535} value={draft.port} onChange={(event) => setDraft({ ...draft, port: Number(event.target.value) })} /></label>
        <label className="enabled-field"><input type="checkbox" checked={draft.enabled} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} /> {t("sources.enabled")}</label>
        <button className="primary-button" disabled={saving}><Plus size={16} /> {t("sources.add")}</button>
      </form>
      {error && <div className="error-strip"><AlertTriangle size={16} /> {error}</div>}
      <section className="source-tools" aria-label={t("sources.controls")}>
        <div className="search-box">
          <Search size={16} />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("sources.search")}
            aria-label={t("sources.search")}
          />
          {query && <button type="button" title={t("common.clearSearch")} aria-label={t("common.clearSearch")} onClick={() => setQuery("")}><X size={15} /></button>}
        </div>
        <label className="source-sort">
          <ArrowUpDown size={15} />
          <select value={sort} onChange={(event) => setSort(event.target.value as SourceSort)} aria-label={t("sources.sort")}>
            <option value="name-asc">{t("sources.nameAsc")}</option>
            <option value="name-desc">{t("sources.nameDesc")}</option>
            <option value="port-asc">{t("sources.portAsc")}</option>
            <option value="port-desc">{t("sources.portDesc")}</option>
          </select>
        </label>
      </section>
      <div className="source-list">
        <div className="source-list-head"><span>{t("sources.name")}</span><span>{t("sources.port")}</span><span>{t("sources.status")}</span><span></span></div>
        {visibleSources.map((source) => (
          <div className="source-row" key={source.id}>
            <span className="source-title"><Network size={16} /> {source.name}</span>
            <span className="mono">{source.port}</span>
            <button className={source.enabled ? "status-toggle enabled" : "status-toggle"} onClick={() => void toggle(source)}>
              <span>{source.enabled ? <Check size={12} /> : null}</span>{source.enabled ? t("sources.enabled") : t("sources.disabled")}
            </button>
            <button className="icon-button danger" title={t("sources.delete")} aria-label={t("sources.delete")} onClick={() => void remove(source)}><Trash2 size={16} /></button>
          </div>
        ))}
        {visibleSources.length === 0 && (
          <div className="empty-state">
            {sources.length === 0 ? <Network size={20} /> : <ListFilter size={20} />}
            <strong>{sources.length === 0 ? t("sources.empty") : t("sources.noMatch")}</strong>
          </div>
        )}
      </div>
    </>
  );
}

function SessionDetail({ session, onClose }: { session: Session; onClose: () => void }) {
  const { locale, t } = useI18n();
  const [payloads, setPayloads] = useState<{ c2s: Uint8Array; s2c: Uint8Array } | null>(null);
  const [matches, setMatches] = useState<FlagMatches>({ c2s: [], s2c: [] });
  const [mode, setMode] = useState<PayloadMode>("text");
  const [formatJson, setFormatJson] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const panelRef = useRef<HTMLElement>(null);

  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    const containWheel = (event: WheelEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      const scrollable = target?.closest<HTMLElement>(".stream pre");
      const canScroll = scrollable && (
        (event.deltaY < 0 && scrollable.scrollTop > 0)
        || (event.deltaY > 0 && scrollable.scrollTop + scrollable.clientHeight < scrollable.scrollHeight)
      );
      if (!canScroll) event.preventDefault();
      event.stopPropagation();
    };
    panel.addEventListener("wheel", containWheel, { passive: false });
    return () => panel.removeEventListener("wheel", containWheel);
  }, []);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  useEffect(() => {
    let active = true;
    Promise.all([api.getPayload(session.id, "c2s"), api.getPayload(session.id, "s2c"), api.getFlagMatches(session.id)])
      .then(([c2s, s2c, found]) => { if (active) { setPayloads({ c2s, s2c }); setMatches(found); } })
      .catch((caught) => { if (active) setError(messageOf(caught, t("error.unexpected"))); });
    return () => { active = false; };
  }, [session.id, t]);

  return (
    <aside
      ref={panelRef}
      className="detail-panel"
      aria-label={t("detail.label")}
    >
      <header className="detail-header">
        <div><span className="eyebrow">{t("detail.session", { id: session.id })}</span><h2>{session.http.method ? `${session.http.method} ${decodeDisplayEscapes(session.http.path ?? "")}` : protocolLabel(session.protocol, t("sessions.rawTcp"))}</h2></div>
        <div className="detail-close">
          <span className="close-hint"><kbd>Esc</kbd> {t("detail.closeHint")}</span>
          <button className="icon-button" title={t("detail.close")} aria-label={t("detail.close")} onClick={onClose}><X size={18} /></button>
        </div>
      </header>
      <div className="detail-meta">
        <span><small>{t("common.source")}</small>{session.source_name} :{session.server_port}</span>
        <span><small>{t("detail.started")}</small>{formatDateTime(session.started_at, locale)}</span>
        <span><small>{t("detail.traffic")}</small>{formatBytes(session.bytes_c2s + session.bytes_s2c)}</span>
        <span><small>{t("common.protocol")}</small>{protocolLabel(session.protocol, t("sessions.rawTcp"))}</span>
      </div>
      {session.contains_flag && <div className="flag-banner"><Flag size={16} /><strong>{t("detail.flagMatches", { count: session.flag_count })}</strong><span>{session.flag_direction.toUpperCase()}</span></div>}
      {error && <div className="error-strip"><AlertTriangle size={16} /> {error}</div>}
      <div className="payload-toolbar">
        <label className={mode === "text" ? "json-toggle" : "json-toggle disabled"}>
          <input
            type="checkbox"
            checked={formatJson}
            disabled={mode !== "text"}
            onChange={(event) => setFormatJson(event.target.checked)}
          />
          {t("detail.formatJson")}
        </label>
        <div className="segmented" role="group" aria-label={t("detail.payloadFormat")}>
          <button className={mode === "text" ? "active" : ""} onClick={() => setMode("text")}><Braces size={14} /> {t("detail.text")}</button>
          <button className={mode === "hex" ? "active" : ""} onClick={() => setMode("hex")}><Settings2 size={14} /> {t("detail.hex")}</button>
        </div>
      </div>
      {!payloads ? <div className="payload-loading"><RefreshCw className="spin" size={17} /></div> : (
        <div className="streams">
          <Stream title={t("detail.clientToServer")} bytes={payloads.c2s} ranges={matches.c2s} mode={mode} formatJson={formatJson} flagged={session.flag_direction === "c2s" || session.flag_direction === "both"} />
          <Stream title={t("detail.serverToClient")} bytes={payloads.s2c} ranges={matches.s2c} mode={mode} formatJson={formatJson} flagged={session.flag_direction === "s2c" || session.flag_direction === "both"} />
        </div>
      )}
    </aside>
  );
}

function Stream({ title, bytes, ranges, mode, formatJson, flagged }: { title: string; bytes: Uint8Array; ranges: ByteRange[]; mode: PayloadMode; formatJson: boolean; flagged: boolean }) {
  const { t } = useI18n();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const copyResetRef = useRef<number | null>(null);
  const text = useMemo(
    () => mode === "hex" ? toHex(bytes) : displayPayload(bytes, formatJson),
    [bytes, formatJson, mode],
  );
  const matchTexts = useMemo(
    () => ranges.map((range) => decodeDisplayEscapes(decodePayload(bytes.slice(range.start, range.end)))).filter(Boolean),
    [bytes, ranges],
  );

  useEffect(() => () => {
    if (copyResetRef.current !== null) window.clearTimeout(copyResetRef.current);
  }, []);

  const copy = async () => {
    try {
      await copyText(text);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
    if (copyResetRef.current !== null) window.clearTimeout(copyResetRef.current);
    copyResetRef.current = window.setTimeout(() => setCopyState("idle"), 1_500);
  };

  return (
    <section className={flagged ? "stream flagged" : "stream"}>
      <header>
        <span>{title}</span>
        <span className="stream-actions">
          <span className="mono">{formatBytes(bytes.length)}</span>
          <button className={copyState === "failed" ? "copy-failed" : ""} type="button" title={copyState === "copied" ? t("detail.copied") : copyState === "failed" ? t("detail.copyFailed") : t("detail.copy", { title })} aria-label={t("detail.copy", { title })} onClick={() => void copy()}>
            {copyState === "copied" ? <Check size={14} /> : copyState === "failed" ? <AlertTriangle size={14} /> : <Copy size={14} />}
          </button>
        </span>
      </header>
      <pre>{mode === "text" && matchTexts.length > 0 ? <HighlightedText text={text} matches={matchTexts} /> : text}</pre>
    </section>
  );
}

function HighlightedText({ text, matches }: { text: string; matches: string[] }) {
  const positions: ByteRange[] = [];
  const cursors = new Map<string, number>();
  for (const match of matches) {
    const start = text.indexOf(match, cursors.get(match) ?? 0);
    if (start < 0) continue;
    positions.push({ start, end: start + match.length });
    cursors.set(match, start + match.length);
  }
  positions.sort((left, right) => left.start - right.start);

  const chunks: ReactNode[] = [];
  let offset = 0;
  for (const range of positions) {
    if (range.start < offset) continue;
    if (range.start > offset) chunks.push(text.slice(offset, range.start));
    chunks.push(<mark key={`${range.start}-${range.end}`}>{text.slice(range.start, range.end)}</mark>);
    offset = range.end;
  }
  if (offset < text.length) chunks.push(text.slice(offset));
  return <>{chunks}</>;
}

async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // The legacy path still works on browsers that restrict Clipboard API to HTTPS.
    }
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) throw new Error("Clipboard access was denied");
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

function protocolLabel(protocol: Protocol, rawTcpLabel: string): string {
  return protocol === "raw_tcp" ? rawTcpLabel : protocol === "websocket" ? "WebSocket" : "HTTP";
}

function formatEndpoint(ip: string, port: number): string {
  return ip.includes(":") ? `[${ip}]:${port}` : `${ip}:${port}`;
}

function formatTime(micros: number, locale: string): string {
  return new Date(micros / 1_000).toLocaleTimeString(locale, { hour12: false });
}

function formatDateTime(micros: number, locale: string): string {
  return new Date(micros / 1_000).toLocaleString(locale, { hour12: false });
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

function messageOf(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}
