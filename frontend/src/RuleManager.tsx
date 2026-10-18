import { useEffect, useRef, useState } from "react";
import { Plus, Trash2, X } from "lucide-react";
import { api } from "./api";
import { messageOf } from "./format";
import { useI18n } from "./i18n";
import type { AttackRule, RuleCondition, RuleField, RuleInput, Source } from "./types";

const condition = (): RuleCondition => ({ field: "path", op: "equals", value: "" });
const fresh = (source: number): RuleInput => ({ source_id: source, label: "", enabled: true, adjacent: true, max_gap: 0, steps: [[condition()]] });
const fields: RuleField[] = ["method", "path", "target", "query", "request_body", "status", "response_body"];
const inputOf = ({ id: _id, owner: _owner, ...input }: AttackRule): RuleInput => input;

export function RuleManager({ sources, onChanged, onClose }: { sources: Source[]; onChanged: () => void; onClose: () => void }) {
  const { t } = useI18n();
  const [rules, setRules] = useState<AttackRule[]>([]);
  const [draft, setDraft] = useState<RuleInput>(() => fresh(sources[0]?.id ?? 0));
  const [editing, setEditing] = useState<number>();
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => { document.body.style.overflow = previous; };
  }, []);
  useEffect(() => {
    let current = true;
    api.listRules().then((items) => { if (current) setRules(items); })
      .catch((e) => { if (current) setError(messageOf(e, t("error.unexpected"))); })
      .finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [t]);
  function updateCondition(step: number, index: number, patch: Partial<RuleCondition>) {
    setDraft((current) => ({ ...current, steps: current.steps.map((conditions, s) => s === step ? conditions.map((value, c) => c === index ? { ...value, ...patch } : value) : conditions) }));
  }
  async function save() {
    if (busy) return;
    setBusy(true); setError("");
    try {
      const rule = await api.saveRule(draft, editing);
      setRules((current) => [...current.filter((r) => r.id !== rule.id), rule].sort((a, b) => a.id - b.id));
      setEditing(rule.id); setDraft(inputOf(rule)); onChanged();
    } catch (e) { setError(messageOf(e, t("error.unexpected"))); }
    finally { setBusy(false); }
  }
  async function remove(rule: AttackRule) {
    setBusy(true); setError("");
    try {
      await api.deleteRule(rule.id); setRules((current) => current.filter((r) => r.id !== rule.id));
      if (editing === rule.id) { setEditing(undefined); setDraft(fresh(sources[0]?.id ?? 0)); }
      onChanged();
    } catch (e) { setError(messageOf(e, t("error.unexpected"))); }
    finally { setBusy(false); }
  }
  return <dialog ref={dialog} className="rule-dialog" onCancel={(e) => { if (busy) e.preventDefault(); else onClose(); }}>
    <header><h2>{t("rules.title")}</h2><button className="icon-button" disabled={busy} aria-label={t("rules.close")} onClick={onClose}><X size={18} /></button></header>
    <p>{t("rules.description")}</p>
    {error && <div className="error-strip" role="alert">{error}</div>}
    <div className="rule-layout">
      <aside><button className="secondary-button" disabled={busy} onClick={() => { setEditing(undefined); setDraft(fresh(sources[0]?.id ?? 0)); }}><Plus size={14} /> {t("rules.new")}</button>
        {loading && <p role="status">{t("rules.loading")}</p>}
        {!loading && rules.length === 0 && <p>{t("rules.empty")}</p>}
        {rules.map((rule) => <div className={`rule-list-item ${editing === rule.id ? "active" : ""}`} key={rule.id}>
          <button disabled={busy} onClick={() => { setEditing(rule.id); setDraft(inputOf(structuredClone(rule))); setError(""); }}>
            <strong>{rule.label}</strong><small>{sources.find((s) => s.id === rule.source_id)?.name ?? `#${rule.source_id}`} · {rule.owner}{!rule.enabled && ` · ${t("rules.disabled")}`}</small>
          </button><button className="icon-button" disabled={busy} aria-label={t("rules.delete", { label: rule.label })} onClick={() => void remove(rule)}><Trash2 size={14} /></button>
        </div>)}
      </aside>
      <form onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <fieldset disabled={busy}>
          <label>{t("rules.label")}<input required maxLength={160} value={draft.label} onChange={(e) => setDraft({ ...draft, label: e.target.value })} /></label>
          <label>{t("rules.source")}<select required value={draft.source_id || ""} onChange={(e) => setDraft({ ...draft, source_id: Number(e.target.value) })}>
            <option value="" disabled>{t("rules.chooseSource")}</option>{draft.source_id > 0 && !sources.some((s) => s.id === draft.source_id) && <option value={draft.source_id}>#{draft.source_id}</option>}{sources.map((s) => <option value={s.id} key={s.id}>{s.name} :{s.port}</option>)}
          </select></label>
          <label className="rule-checkbox"><input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />{t("rules.enabled")}</label>
          <label className="rule-checkbox"><input type="checkbox" checked={draft.adjacent} onChange={(e) => setDraft({ ...draft, adjacent: e.target.checked, max_gap: 0 })} />{t("rules.adjacent")}</label>
          {!draft.adjacent && <label>{t("rules.maxGap")}<input type="number" min={0} max={100} required value={draft.max_gap} onChange={(e) => setDraft({ ...draft, max_gap: Number(e.target.value) })} /></label>}
          {draft.steps.map((conditions, step) => <section className="rule-step" key={step}>
            <header><strong>{step ? `${t("rules.then")} · ` : ""}{t("rules.step", { count: step + 1 })}</strong>
              {draft.steps.length > 1 && <button type="button" aria-label={t("rules.removeStep")} onClick={() => setDraft({ ...draft, steps: draft.steps.filter((_, i) => i !== step) })}><X size={14} /></button>}</header>
            {conditions.map((value, index) => <div className="rule-condition" key={index}>
              <select aria-label={t("rules.field")} value={value.field} onChange={(e) => updateCondition(step, index, { field: e.target.value as RuleField, name: undefined })}>
                {fields.map((field) => <option value={field} key={field}>{t(`rules.field.${field}`)}</option>)}
              </select>
              <select aria-label={t("rules.operator")} value={value.op} onChange={(e) => updateCondition(step, index, { op: e.target.value as RuleCondition["op"] })}>
                {(["equals", "contains", "regex"] as const).map((op) => <option key={op} value={op}>{t(`rules.op.${op}`)}</option>)}
              </select>
              {value.field === "query" && <input aria-label={t("rules.queryName")} placeholder={t("rules.queryName")} maxLength={128} value={value.name ?? ""} onChange={(e) => updateCondition(step, index, { name: e.target.value || undefined })} />}
              {value.field === "request_body" || value.field === "response_body"
                ? <textarea aria-label={t("rules.value")} placeholder={t("rules.value")} required={value.op !== "equals"} rows={2} maxLength={2048} value={value.value} onChange={(e) => updateCondition(step, index, { value: e.target.value })} />
                : <input aria-label={t("rules.value")} placeholder={t("rules.value")} required={value.op !== "equals"} maxLength={2048} value={value.value} onChange={(e) => updateCondition(step, index, { value: e.target.value })} />}
              {conditions.length > 1 && <button type="button" aria-label={t("rules.removeCondition")} onClick={() => setDraft({ ...draft, steps: draft.steps.map((items, s) => s === step ? items.filter((_, c) => c !== index) : items) })}><X size={14} /></button>}
            </div>)}
            <button className="secondary-button" type="button" disabled={conditions.length >= 12} onClick={() => setDraft({ ...draft, steps: draft.steps.map((items, s) => s === step ? [...items, condition()] : items) })}>{t("rules.and")}</button>
          </section>)}
          <footer><button className="secondary-button" type="button" disabled={draft.steps.length >= 8} onClick={() => setDraft({ ...draft, steps: [...draft.steps, [condition()]] })}>{t("rules.addStep")}</button>
            <button className="primary-button" disabled={!draft.source_id} type="submit">{busy ? t("rules.saving") : t("rules.save")}</button></footer>
        </fieldset>
      </form>
    </div>
  </dialog>;
}
