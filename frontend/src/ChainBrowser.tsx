import { AlertTriangle, Braces, Flag, ListFilter, RefreshCw, Settings2, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "./api";
import { formatBytes, formatDateTime, formatTime, messageOf } from "./format";
import { useI18n } from "./i18n";
import { Stream } from "./Stream";
import type { PayloadMode } from "./Stream";
import { TrafficFilters } from "./TrafficFilters";
import type { TrafficFilterProps } from "./TrafficFilters";
import type { Chain, FlagMatches, Session } from "./types";

export function ChainBrowser(props: TrafficFilterProps & { onUngroup: () => void }) {
  const { locale, t } = useI18n();
  const [chains, setChains] = useState<Chain[]>([]);
  const [selected, setSelected] = useState<Chain | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rowsRef = useRef<Chain[]>([]);
  const cursorRef = useRef<string | null>(null);
  const generation = useRef(0);
  const requestRef = useRef<AbortController | null>(null);
  const sentinel = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ id: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (anchor.current) {
      const row = document.querySelector(`[data-chain-id="${anchor.current.id}"]`);
      if (row) window.scrollBy(0, row.getBoundingClientRect().top - anchor.current.top);
      anchor.current = null;
    }
  }, [chains]);

  const load = useCallback(async (refresh: boolean) => {
    if (requestRef.current || (!refresh && !cursorRef.current)) return;
    const controller = new AbortController();
    requestRef.current = controller;
    const currentGeneration = generation.current;
    setLoading(true);
    setError(null);
    try {
      let page = await api.listChains(props.filters, refresh ? undefined : cursorRef.current ?? undefined, controller.signal);
      let incoming = page.items;
      // Refresh the loaded span against one new snapshot. A late session can
      // merge two old rows; merging summaries only by ID would leave stale chains.
      const desired = refresh ? rowsRef.current.length : 0;
      while (refresh && page.next_cursor && incoming.length < desired) {
        const previous = page.next_cursor;
        page = await api.listChains(props.filters, previous, controller.signal);
        if (page.next_cursor === previous) throw new Error(t("history.noProgress"));
        incoming = [...incoming, ...page.items];
      }
      if (currentGeneration !== generation.current || controller.signal.aborted) return;
      if (!refresh && page.next_cursor === cursorRef.current) throw new Error(t("history.noProgress"));
      if (refresh && window.scrollY > 80) {
        const row = Array.from(document.querySelectorAll<HTMLElement>("[data-chain-id]")).find((row) => row.getBoundingClientRect().bottom > 0);
        if (row) anchor.current = { id: Number(row.dataset.chainId), top: row.getBoundingClientRect().top };
      }
      const merged = new Map((refresh ? [] : rowsRef.current).map((chain) => [chain.id, chain]));
      incoming.forEach((chain) => merged.set(chain.id, chain));
      const rows = Array.from(merged.values()).sort((a, b) => b.started_at - a.started_at || b.id - a.id);
      rowsRef.current = rows;
      cursorRef.current = page.next_cursor;
      setChains(rows);
      setCursor(page.next_cursor);
    } catch (caught) {
      if (!controller.signal.aborted && currentGeneration === generation.current) setError(messageOf(caught, t("error.unexpected")));
    } finally {
      if (requestRef.current === controller) { requestRef.current = null; setLoading(false); }
    }
  }, [props.filters, t]);

  useEffect(() => {
    generation.current++;
    requestRef.current?.abort();
    requestRef.current = null;
    rowsRef.current = [];
    cursorRef.current = null;
    setChains([]);
    setCursor(null);
    setSelected(null);
    window.scrollTo(0, 0);
    void load(true);
    return () => { generation.current++; requestRef.current?.abort(); requestRef.current = null; };
  }, [load]);

  useEffect(() => {
    const node = sentinel.current;
    if (!node || !cursor || loading || error) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void load(false);
    }, { rootMargin: "0px 0px 480px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, [cursor, loading, error, load]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (!selected && !props.filters.payload && window.scrollY <= 80) void load(true);
    }, 5_000);
    return () => window.clearInterval(timer);
  }, [load, selected, props.filters.payload]);

  return <>
    <header className="page-header">
      <div><h1>{t("nav.sessions")}</h1><p>{t("chains.loaded", { count: chains.length })}</p></div>
      <button className="icon-button" title={t("sessions.refresh")} aria-label={t("sessions.refresh")} disabled={loading} onClick={() => void load(true)}><RefreshCw size={17} className={loading ? "spin" : ""} /></button>
    </header>
    <TrafficFilters {...props} grouped onToggleGroup={props.onUngroup} />
    {error && <div className="error-strip"><AlertTriangle size={16} />{error}<button onClick={() => void load(cursor ? false : true)}>{t("common.retry")}</button></div>}
    <div className="table-wrap"><table className="session-table chain-table">
      <thead><tr><th>{t("sessions.time")}</th><th>{t("common.source")}</th><th>{t("sessions.client")}</th><th>{t("chains.collector")}</th><th>{t("chains.steps")}</th><th>{t("chains.duration")}</th><th>{t("sessions.size")}</th><th>{t("sessions.flag")}</th></tr></thead>
      <tbody>{chains.map((chain) => <tr key={chain.id} data-chain-id={chain.id}
        className={[chain.contains_flag ? "flag-row" : "", selected?.id === chain.id ? "selected-row" : ""].filter(Boolean).join(" ")}
        onClick={() => setSelected(chain)}>
        <td className="mono time-cell">{formatTime(chain.started_at, locale)}</td>
        <td><span className="source-name">{chain.source_name}</span></td><td className="mono endpoint">{chain.client_ip}</td>
        <td>{chain.collector_id ?? t("chains.unknown")}</td><td className="mono">{chain.session_count}</td>
        <td className="mono">{((chain.ended_at - chain.started_at) / 1_000_000).toFixed(3)} s</td>
        <td className="mono">{formatBytes(chain.bytes_c2s + chain.bytes_s2c)}</td>
        <td><div className="signals">
          {chain.contains_flag && <span className="flag-signal" title={t("sessions.flagMatches", { count: chain.flag_count })}><Flag size={14} />{chain.flag_count}</span>}
          {chain.suricata_alerts > 0 && <span className="alert-signal" title={t("sessions.suricataAlerts")}><AlertTriangle size={14} />{chain.suricata_alerts}</span>}
          {chain.incomplete && <span className="incomplete-dot" title={t("sessions.incomplete")} />}
        </div></td>
      </tr>)}</tbody>
    </table>{!loading && !chains.length && !error && <div className="empty-state"><ListFilter size={20} /><strong>{t("chains.empty")}</strong></div>}</div>
    <div ref={sentinel} className="history-sentinel" role="status" aria-live="polite">{loading ? t("chains.loading") : !cursor && chains.length ? t("chains.allShown") : null}</div>
    {selected && <ChainDetail key={`${selected.id}:${selected.snapshot_id}`} chain={selected} onClose={() => setSelected(null)} />}
  </>;
}

function ChainDetail({ chain, onClose }: { chain: Chain; onClose: () => void }) {
  const { locale, t } = useI18n();
  const [members, setMembers] = useState<Session[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<PayloadMode>("text");
  const [formatJson, setFormatJson] = useState(false);
  const panel = useRef<HTMLElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const sentinel = useRef<HTMLDivElement>(null);
  const request = useRef<AbortController | null>(null);
  const cursorRef = useRef<string | null>(null);
  const initialized = useRef(false);

  const load = useCallback(async () => {
    if (request.current || (initialized.current && cursorRef.current === null)) return;
    const controller = new AbortController();
    request.current = controller;
    setLoading(true);
    setError(null);
    try {
      const page = await api.getChainMembers(chain.id, chain.snapshot_id, cursorRef.current ?? undefined, controller.signal);
      if (controller.signal.aborted) return;
      if (page.next_cursor && page.next_cursor === cursorRef.current) throw new Error(t("history.noProgress"));
      setMembers((current) => {
        const merged = new Map(current.map((member) => [member.id, member]));
        page.items.forEach((member) => merged.set(member.id, member));
        return Array.from(merged.values()).sort((a, b) => a.started_at - b.started_at || a.id - b.id);
      });
      initialized.current = true;
      cursorRef.current = page.next_cursor;
      setCursor(page.next_cursor);
    } catch (caught) {
      if (!controller.signal.aborted) setError(messageOf(caught, t("error.unexpected")));
    } finally {
      if (request.current === controller) { request.current = null; setLoading(false); }
    }
  }, [chain.id, chain.snapshot_id, t]);

  useEffect(() => { void load(); return () => { request.current?.abort(); request.current = null; }; }, [load]);
  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [onClose]);

  useLayoutEffect(() => {
    const body = scroll.current;
    if (!body) return;
    const size = () => body.style.setProperty("--payload-window-height", `${Math.max(180, (body.clientHeight - 32) / 2)}px`);
    const observer = new ResizeObserver(size);
    observer.observe(body);
    size();
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const node = panel.current;
    if (!node) return;
    const containWheel = (event: WheelEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      const inner = target?.closest<HTMLElement>(".stream pre");
      const area = inner ?? (target?.closest(".chain-scroll") ? scroll.current : null);
      const canScroll = area && ((event.deltaY < 0 && area.scrollTop > 0) || (event.deltaY > 0 && area.scrollTop + area.clientHeight < area.scrollHeight));
      if (!canScroll) event.preventDefault();
      event.stopPropagation();
    };
    node.addEventListener("wheel", containWheel, { passive: false });
    return () => node.removeEventListener("wheel", containWheel);
  }, []);

  useEffect(() => {
    const node = sentinel.current;
    if (!node || !cursor || loading || error) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void load();
    }, { root: scroll.current, rootMargin: "400px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, [cursor, loading, error, load]);

  return <aside ref={panel} className="detail-panel chain-panel" aria-label={t("chains.detail")}>
    <header className="detail-header"><div><span className="eyebrow">{t("chains.title", { count: chain.session_count })}</span><h2>{chain.source_name} · {chain.client_ip}</h2></div>
      <div className="detail-close"><span className="close-hint"><kbd>Esc</kbd> {t("detail.closeHint")}</span><button className="icon-button" title={t("detail.close")} aria-label={t("detail.close")} onClick={onClose}><X size={18} /></button></div>
    </header>
    <div className="detail-meta"><span><small>{t("chains.collector")}</small>{chain.collector_id ?? t("chains.unknown")}</span><span><small>{t("detail.started")}</small>{formatDateTime(chain.started_at, locale)}</span><span><small>{t("detail.traffic")}</small>{formatBytes(chain.bytes_c2s + chain.bytes_s2c)}</span><span><small>{t("chains.steps")}</small>{chain.session_count}</span></div>
    {chain.contains_flag && <div className="flag-banner"><Flag size={16} /><strong>{t("detail.flagMatches", { count: chain.flag_count })}</strong></div>}
    <div className="payload-toolbar"><label className={mode === "text" ? "json-toggle" : "json-toggle disabled"}><input type="checkbox" checked={formatJson} disabled={mode !== "text"} onChange={(event) => setFormatJson(event.target.checked)} />{t("detail.formatJson")}</label>
      <div className="segmented" role="group" aria-label={t("detail.payloadFormat")}><button className={mode === "text" ? "active" : ""} onClick={() => setMode("text")}><Braces size={14} />{t("detail.text")}</button><button className={mode === "hex" ? "active" : ""} onClick={() => setMode("hex")}><Settings2 size={14} />{t("detail.hex")}</button></div>
    </div>
    <div ref={scroll} className="chain-scroll">
      {members.map((session) => <LazyPair key={session.id} session={session} root={scroll.current} mode={mode} formatJson={formatJson} />)}
      {error && <div className="error-strip"><AlertTriangle size={16} />{error}<button onClick={() => void load()}>{t("common.retry")}</button></div>}
      <div ref={sentinel} className="history-sentinel" role="status">{loading ? t("chains.loadingMembers") : cursor ? <button onClick={() => void load()}>{t("chains.more")}</button> : members.length ? t("chains.membersLoaded", { count: members.length }) : null}</div>
    </div>
  </aside>;
}

function LazyPair({ session, root, mode, formatJson }: { session: Session; root: HTMLElement | null; mode: PayloadMode; formatJson: boolean }) {
  const { locale, t } = useI18n();
  const node = useRef<HTMLDivElement>(null);
  const positions = useRef([0, 0]);
  const [visible, setVisible] = useState(false);
  const [payload, setPayload] = useState<{ c2s: Uint8Array; s2c: Uint8Array; matches: FlagMatches } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!node.current) return;
    const observer = new IntersectionObserver((entries) => setVisible(entries.some((entry) => entry.isIntersecting)), { root, rootMargin: "200px" });
    observer.observe(node.current);
    return () => observer.disconnect();
  }, [root]);
  useEffect(() => {
    setPayload(null);
    setError(null);
    if (!visible) return;
    const controller = new AbortController();
    Promise.all([api.getPayload(session.id, "c2s", controller.signal), api.getPayload(session.id, "s2c", controller.signal), api.getFlagMatches(session.id, controller.signal)])
      .then(([c2s, s2c, matches]) => { if (!controller.signal.aborted) setPayload({ c2s, s2c, matches }); })
      .catch((caught) => { if (!controller.signal.aborted) setError(messageOf(caught, t("error.unexpected"))); });
    return () => controller.abort();
  }, [session.id, visible, retry, t]);
  useLayoutEffect(() => {
    const windows = Array.from(node.current?.querySelectorAll<HTMLPreElement>(".stream pre") ?? []);
    windows.forEach((window, index) => { window.scrollTop = positions.current[index]; });
    return () => { if (windows.length) positions.current = windows.map((window) => window.scrollTop); };
  }, [payload, mode]);
  const timestamp = (value: number | null) => value == null ? t("chains.timeUnknown") : new Date(value / 1_000).toLocaleTimeString(locale, { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 3 });
  return <div ref={node} className="chain-pair" data-session-id={session.id}>
    {payload ? <>
      <Stream title={`#${session.id} · C2S`} subtitle={timestamp(session.first_payload_c2s_at)} bytes={payload.c2s} ranges={payload.matches.c2s} mode={mode} formatJson={formatJson} flagged={session.flag_direction === "c2s" || session.flag_direction === "both"} replayTarget={{ host: session.server_ip, port: session.server_port }} />
      <Stream title={`#${session.id} · S2C`} subtitle={timestamp(session.first_payload_s2c_at)} bytes={payload.s2c} ranges={payload.matches.s2c} mode={mode} formatJson={formatJson} flagged={session.flag_direction === "s2c" || session.flag_direction === "both"} />
    </> : ["C2S", "S2C"].map((direction) => <section key={direction} className="stream"><header><span>#{session.id} · {direction}</span></header><div className="payload-loading">{error ? <button onClick={() => setRetry((value) => value + 1)} title={error}>{t("common.retry")}</button> : visible ? <RefreshCw className="spin" size={17} /> : null}</div></section>)}
  </div>;
}
