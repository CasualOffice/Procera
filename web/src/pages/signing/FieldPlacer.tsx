import { Plus, Trash2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Input, Label, NumberField, Radio, RadioGroup } from 'react-aria-components';
import { type Envelope, FIELD_KINDS, type FieldKind, type SignField, kindLabel } from '../../lib/signing';
import { Button } from '../../ui/components/Button';
import { Checkbox } from '../../ui/components/Checkbox';
import { Select } from '../../ui/components/Select';
import { TextField } from '../../ui/components/TextField';
import { PdfPages } from './PdfPages';
import { hueOf, initials, signers } from './shared';

/** A field being edited: the server's shape plus a stable client key (ids are assigned on save). */
export type LocalField = SignField & { key: string };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const r4 = (v: number) => Math.round(v * 10000) / 10000;
/** Keeps a box inside its page (fractions of the page from its top-left). */
export function fit(f: LocalField): LocalField {
  const w = r4(clamp(f.w, 0.01, 1));
  const h = r4(clamp(f.h, 0.01, 1));
  return { ...f, w, h, x: r4(clamp(f.x, 0, 1 - w)), y: r4(clamp(f.y, 0, 1 - h)) };
}
const fieldName = (f: LocalField) => f.label?.trim() || kindLabel(f.kind);

/**
 * Fields (docs/05 §3.22 step 4): pick who fills it and what it is, then click a page to place a box
 * there, or "Add to page" from the keyboard. A placed box is a button: arrow keys move it (Shift for
 * bigger steps), Delete removes it, and the panel beside the document edits it. No dragging needed
 * (WCAG 2.5.7). Colour is never the only cue: boxes carry the signer's initials and the field name.
 */
export function FieldPlacer({ env, fields, onChange, pdf, pdfError }: {
  env: Envelope; fields: LocalField[]; onChange: (next: LocalField[]) => void; pdf: ArrayBuffer | null; pdfError: string | null;
}) {
  const who = signers(env);
  const [recipient, setRecipient] = useState<string>(who[0]?.id ?? '');
  const [kind, setKind] = useState<FieldKind>('signature');
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<string | null>(null);
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const [said, setSaid] = useState('');
  const boxes = useRef(new Map<string, HTMLButtonElement>());
  const pages = env.pages.length;

  // The chosen signer may have been removed or made a copy recipient.
  const whoKey = who.map((r) => r.id).join(',');
  useEffect(() => { if (!who.some((r) => r.id === recipient)) setRecipient(who[0]?.id ?? ''); }, [whoKey, recipient]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (selected && !fields.some((f) => f.key === selected)) setSelected(null); }, [fields, selected]);
  useEffect(() => {
    if (!focusKey) return;
    const el = boxes.current.get(focusKey);
    if (el) { el.focus({ preventScroll: false }); el.scrollIntoView({ block: 'nearest' }); setFocusKey(null); }
  });

  const nameOf = (id: string) => env.recipients.find((r) => r.id === id)?.name ?? 'Unknown';
  const update = (key: string, fn: (f: LocalField) => LocalField) => onChange(fields.map((f) => (f.key === key ? fit(fn(f)) : f)));
  const remove = (f: LocalField) => {
    onChange(fields.filter((x) => x.key !== f.key));
    setSaid(`${fieldName(f)} for ${nameOf(f.recipient_id)} removed.`);
  };
  const add = (p: number, cx: number, cy: number) => {
    if (!recipient) return;
    const k = FIELD_KINDS.find((x) => x.id === kind)!;
    const f = fit({ key: crypto.randomUUID(), recipient_id: recipient, kind, page: p, x: cx - k.w / 2, y: cy - k.h / 2, w: k.w, h: k.h, required: true, label: null });
    onChange([...fields, f]);
    setSelected(f.key);
    setFocusKey(f.key);
    setSaid(`${k.label} for ${nameOf(recipient)} placed on page ${p}.`);
  };
  const onBoxKey = (e: React.KeyboardEvent, f: LocalField) => {
    const step = e.shiftKey ? 0.05 : 0.005;
    const moves: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    const m = moves[e.key];
    if (m) { e.preventDefault(); setSelected(f.key); update(f.key, (x) => ({ ...x, x: x.x + m[0], y: x.y + m[1] })); return; }
    if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); remove(f); return; }
    if (e.key === 'Escape') setSelected(null);
  };

  if (!who.length) return <p className="muted sign-empty-note">Add at least one person who signs, then place where they sign.</p>;
  const sel = fields.find((f) => f.key === selected) ?? null;
  const kindName = kindLabel(kind);

  return (
    <div className="placer">
      <div className="placer-palette">
        <RadioGroup className="palette-group" value={recipient} onChange={setRecipient}>
          <Label className="palette-label">Who fills it</Label>
          <div className="palette-row">
            {who.map((r) => (
              <Radio key={r.id} value={r.id} className="palette-chip">
                <span className="sign-swatch" style={{ background: hueOf(env.recipients, r.id) }} aria-hidden="true">{initials(r.name)}</span>{r.name}
              </Radio>
            ))}
          </div>
        </RadioGroup>
        <RadioGroup className="palette-group" value={kind} onChange={(v) => setKind(v as FieldKind)}>
          <Label className="palette-label">Field type</Label>
          <div className="palette-row">
            {FIELD_KINDS.map((k) => <Radio key={k.id} value={k.id} className="palette-chip">{k.label}</Radio>)}
          </div>
        </RadioGroup>
        <div className="palette-add">
          <NumberField className="palette-page" value={page} minValue={1} maxValue={pages} step={1} onChange={(v) => setPage(Number.isFinite(v) ? v : 1)}>
            <Label className="palette-label">Page</Label>
            <Input className="field-input" />
          </NumberField>
          <Button variant="secondary" onPress={() => add(page, 0.5, 0.5 + ((fields.filter((f) => f.page === page).length % 6) - 2.5) * 0.06)}>
            <Plus aria-hidden="true" size="0.875rem" /> Add to page
          </Button>
        </div>
        <p className="muted placer-hint">Click on the document to place a {kindName.toLowerCase()} for {nameOf(recipient)}. Select a box, then use the arrow keys to move it (Shift moves further) and Delete to remove it.</p>
      </div>

      <div className="placer-body">
        <div className="placer-doc">
          {pdfError ? <p className="field-error" role="alert">{pdfError}</p> : pdf ? (
            <PdfPages data={pdf} label="Document pages" overlay={(p) => (
              <>
                <div className="place-layer" data-place-page={p} aria-hidden="true"
                  onClick={(e) => { const b = e.currentTarget.getBoundingClientRect(); add(p, (e.clientX - b.left) / b.width, (e.clientY - b.top) / b.height); }} />
                {fields.filter((f) => f.page === p).map((f) => {
                  const hue = hueOf(env.recipients, f.recipient_id);
                  const name = nameOf(f.recipient_id);
                  return (
                    <button key={f.key} type="button" ref={(el) => { if (el) boxes.current.set(f.key, el); else boxes.current.delete(f.key); }}
                      className={`field-box${f.key === selected ? ' is-selected' : ''}`} aria-pressed={f.key === selected}
                      aria-label={`${fieldName(f)} for ${name}, page ${p}`}
                      style={{ left: `${f.x * 100}%`, top: `${f.y * 100}%`, width: `${f.w * 100}%`, height: `${f.h * 100}%`, borderColor: hue, background: `color-mix(in srgb, ${hue} 10%, #fff)` }}
                      onClick={(e) => { e.stopPropagation(); setSelected(f.key); }} onKeyDown={(e) => onBoxKey(e, f)}>
                      <span className="field-box-who" style={{ background: hue }} aria-hidden="true">{initials(name)}</span>
                      <span className="field-box-name" aria-hidden="true">{fieldName(f)}</span>
                    </button>
                  );
                })}
              </>
            )} />
          ) : <p className="muted pdf-pages-loading">Loading the document…</p>}
        </div>

        <aside className="placer-side">
          {sel ? (
            <section className="card placer-props" aria-labelledby="sel-h">
              <div className="card-head"><h3 id="sel-h">Selected field</h3>
                <Button variant="ghost" className="btn-sm" aria-label={`Remove ${fieldName(sel)} for ${nameOf(sel.recipient_id)}`} onPress={() => remove(sel)}><Trash2 aria-hidden="true" size="0.875rem" /> Remove</Button>
              </div>
              <div className="card-body stack">
                <Select label="Type" options={FIELD_KINDS.map((k) => ({ id: k.id, label: k.label }))} selectedKey={sel.kind} onSelectionChange={(k) => k && update(sel.key, (f) => ({ ...f, kind: k as FieldKind, required: k === 'signature' ? true : f.required }))} />
                <Select label="Filled by" options={who.map((r) => ({ id: r.id, label: r.name }))} selectedKey={sel.recipient_id} onSelectionChange={(k) => k && update(sel.key, (f) => ({ ...f, recipient_id: String(k) }))} />
                <TextField label="Label" value={sel.label ?? ''} maxLength={100} onChange={(v) => update(sel.key, (f) => ({ ...f, label: v }))} description="Shown to the signer, e.g. “Start date”." />
                <Checkbox isSelected={sel.kind === 'signature' || sel.required} isDisabled={sel.kind === 'signature'} onChange={(v) => update(sel.key, (f) => ({ ...f, required: v }))}>
                  Required{sel.kind === 'signature' && <span className="muted"> (always, for a signature)</span>}
                </Checkbox>
                <div className="placer-dims">
                  <Pct label="Width %" value={sel.w} onChange={(v) => update(sel.key, (f) => ({ ...f, w: v }))} />
                  <Pct label="Height %" value={sel.h} onChange={(v) => update(sel.key, (f) => ({ ...f, h: v }))} />
                  <Pct label="From left %" value={sel.x} onChange={(v) => update(sel.key, (f) => ({ ...f, x: v }))} />
                  <Pct label="From top %" value={sel.y} onChange={(v) => update(sel.key, (f) => ({ ...f, y: v }))} />
                </div>
              </div>
            </section>
          ) : null}

          <section className="placer-list" aria-labelledby="fields-h">
            <h3 id="fields-h">Fields ({fields.length})</h3>
            {fields.length === 0 && <p className="muted">None yet. Every signer needs a signature field.</p>}
            {who.map((r) => {
              const mine = fields.filter((f) => f.recipient_id === r.id).sort((a, b) => a.page - b.page || a.y - b.y);
              if (!mine.length) return null;
              return (
                <div key={r.id} className="placer-group">
                  <p className="placer-group-name"><span className="sign-swatch" style={{ background: hueOf(env.recipients, r.id) }} aria-hidden="true">{initials(r.name)}</span>{r.name}</p>
                  <ul aria-label={`Fields for ${r.name}`}>
                    {mine.map((f) => (
                      <li key={f.key}>
                        <button type="button" className="prop-link" onClick={() => { setSelected(f.key); setFocusKey(f.key); }}>{fieldName(f)} · page {f.page}</button>
                        <button type="button" className="prop-link" aria-label={`Remove ${fieldName(f)} for ${r.name} on page ${f.page}`} onClick={() => remove(f)}>Remove</button>
                      </li>
                    ))}
                  </ul>
                </div>
              );
            })}
          </section>
        </aside>
      </div>
      <p className="visually-hidden" role="status" aria-live="polite">{said}</p>
    </div>
  );
}

/** A percentage of the page (stored as a fraction). */
function Pct({ label, value, onChange }: { label: string; value: number; onChange: (fraction: number) => void }) {
  return (
    <NumberField className="field" value={Math.round(value * 1000) / 10} minValue={0} maxValue={100} step={0.5} formatOptions={{ maximumFractionDigits: 1 }}
      onChange={(v) => Number.isFinite(v) && onChange(v / 100)}>
      <Label className="field-label">{label}</Label>
      <Input className="field-input" />
    </NumberField>
  );
}
