import { Flag, Search, X } from "lucide-react";
import type { FormEvent } from "react";
import { useI18n } from "./i18n";
import type { Protocol, SessionFilters, Source } from "./types";

export type TrafficFilterProps = {
  sources: Source[];
  filters: SessionFilters;
  searchValue: string;
  onFilters: (updater: (current: SessionFilters) => SessionFilters) => void;
  onSearchValue: (value: string) => void;
  onSearch: (event: FormEvent) => void;
};

export function TrafficFilters(props: TrafficFilterProps & { grouped: boolean; onToggleGroup: () => void }) {
  const { t } = useI18n();
  const protocols: Array<{ label: string; value?: Protocol }> = [
    { label: t("sessions.all") }, { label: "HTTP", value: "http" },
    { label: "WebSocket", value: "websocket" }, { label: t("sessions.rawTcp"), value: "raw_tcp" },
  ];
  return <>
    <section className="filter-bar">
      <form className="search-box" onSubmit={props.onSearch}>
        <Search size={16} />
        <input value={props.searchValue} onChange={(event) => props.onSearchValue(event.target.value)} placeholder={t("sessions.searchPayload")} />
        {props.searchValue && <button type="button" title={t("common.clearSearch")} aria-label={t("common.clearSearch")} onClick={() => props.onSearchValue("")}><X size={15} /></button>}
      </form>
      <select aria-label={t("sessions.sourceFilter")} value={props.filters.sourceId ?? ""}
        onChange={(event) => props.onFilters((current) => ({ ...current, sourceId: Number(event.target.value) || undefined }))}>
        <option value="">{t("sessions.allSources")}</option>
        {props.sources.map((source) => <option key={source.id} value={source.id}>{source.name} :{source.port}</option>)}
      </select>
      <label className="flag-toggle"><input type="checkbox" checked={props.filters.containsFlag ?? false}
        onChange={(event) => props.onFilters((current) => ({ ...current, containsFlag: event.target.checked || undefined }))} />
        <Flag size={15} /> {t("sessions.flagsOnly")}
      </label>
      <label className="flag-toggle"><input type="checkbox" checked={props.grouped} onChange={props.onToggleGroup} />{t("chains.group")}</label>
    </section>
    <div className="protocol-tabs" role="tablist" aria-label={t("sessions.protocolFilter")}>
      {protocols.map((protocol) => <button key={protocol.label}
        className={props.filters.protocol === protocol.value ? "active" : ""}
        onClick={() => props.onFilters((current) => ({ ...current, protocol: protocol.value }))}>{protocol.label}</button>)}
    </div>
  </>;
}
