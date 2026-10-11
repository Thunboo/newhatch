import { Flag, Plus, Search, X } from "lucide-react";
import type { FormEvent } from "react";
import { useI18n } from "./i18n";
import type { PayloadSearchLimits, Protocol, SessionFilters, Source } from "./types";
import type { PayloadChip } from "./useTrafficFilters";

export type TrafficFilterProps = {
  sources: Source[];
  filters: SessionFilters;
  searchValue: string;
  onFilters: (updater: (current: SessionFilters) => SessionFilters) => void;
  onSearchValue: (value: string) => void;
  onSearch: (event: FormEvent) => void;
  chips: PayloadChip[];
  limits: PayloadSearchLimits | null;
  limitsError: string | null;
  filterError: string | null;
  pendingFilters: boolean;
  onRemoveChip: (id: number) => void;
  onRetryLimits: () => void;
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
        <input aria-label={t("sessions.searchPayload")} value={props.searchValue} onChange={(event) => props.onSearchValue(event.target.value)} placeholder={t("sessions.searchPayload")} />
        {props.searchValue && <button type="button" title={t("common.clearSearch")} aria-label={t("common.clearSearch")} onClick={() => props.onSearchValue("")}><X size={15} /></button>}
        <button className="add-filter" type="submit" title={t("filters.add")} aria-label={t("filters.add")} disabled={!props.limits && !!props.searchValue.trim()}><Plus size={15} /><span>{t("filters.add")}</span></button>
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
    {(props.chips.length > 0 || props.pendingFilters) && <div className="payload-filters" aria-label={t("filters.list")}>
      {props.chips.map((chip) => <span className="payload-filter" key={chip.id}>
        <span className="mono" title={chip.term}>{chip.term}</span>
        <button type="button" aria-label={t("filters.remove", { term: chip.term })} onClick={() => props.onRemoveChip(chip.id)}><X size={14} /></button>
      </span>)}
      {props.pendingFilters && <span className="filter-notice" role="status">{t("filters.pending")}</span>}
    </div>}
    {props.filterError && <div className="filter-error" role="alert">{props.filterError}</div>}
    {props.limitsError && <div className="filter-error" role="alert">{t("filters.limitsFailed")} {props.limitsError} <button onClick={props.onRetryLimits}>{t("common.retry")}</button></div>}
    <div className="protocol-tabs" role="tablist" aria-label={t("sessions.protocolFilter")}>
      {protocols.map((protocol) => <button key={protocol.label}
        className={props.filters.protocol === protocol.value ? "active" : ""}
        onClick={() => props.onFilters((current) => ({ ...current, protocol: protocol.value }))}>{protocol.label}</button>)}
    </div>
  </>;
}
