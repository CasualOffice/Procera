import { ArrowRight, Clock, CornerDownLeft, FileText, ScrollText, Search, User, type LucideIcon } from 'lucide-react';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { Dialog, Modal, ModalOverlay } from 'react-aria-components';
import { api } from '../lib/api';
import { type RecentItem, recentItems, rememberRecent } from '../lib/recent';
import { navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { term, useTerms } from '../lib/terms';
import { Avatar } from './Avatar';
import { createActions, navGroups } from './nav';

interface Hit { type: string; id: string; title: string; subtitle: string | null; snippet: string | null }
interface Option { id: string; section: string; label: string; hint?: string; snippet?: string | null; icon?: LucideIcon; avatar?: string; to: string; recent?: RecentItem }

export const isMac = () => typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
export const shortcutLabel = () => (isMac() ? '⌘K' : 'Ctrl K');

const KIND: Record<string, { section: string; icon: LucideIcon; to: (h: Hit) => string; label: string }> = {
  policy: { section: 'Documents', icon: FileText, to: (h) => `/library/${h.id}`, label: 'Policy' },
  sop: { section: 'Documents', icon: ScrollText, to: (h) => `/library/${h.id}`, get label() { return term('sop', { capitalised: true }); } },
  member: { section: 'People', icon: User, to: (h) => `/admin/members?q=${encodeURIComponent(h.title)}`, label: 'Person' },
};

/** Loose subsequence match ("gtm" finds "Go to Members"), word starts first. */
function score(text: string, q: string, loose = true): number {
  const t = text.toLowerCase();
  const s = q.toLowerCase().trim();
  if (!s) return 1;
  if (t.startsWith(s)) return 4;
  if (t.split(/\s+/).some((w) => w.startsWith(s))) return 3;
  if (t.includes(s)) return 2;
  if (!loose || s.length < 2) return 0;
  let i = 0;
  for (const c of t) if (c === s[i]) i++;
  return i === s.length ? 1 : 0;
}

/** `<mark>`-delimited, already-escaped snippet from the search API, rendered without innerHTML. */
function Snippet({ html }: { html: string }) {
  const decode = (s: string) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&');
  const parts = html.split(/(<mark>|<\/mark>)/);
  let on = false;
  return (
    <span className="cmd-snippet">
      {parts.map((p, i) => {
        if (p === '<mark>') { on = true; return null; }
        if (p === '</mark>') { on = false; return null; }
        return on ? <mark key={i}>{decode(p)}</mark> : <span key={i}>{decode(p)}</span>;
      })}
    </span>
  );
}

/**
 * Command palette (docs/05 §2.4): navigation, create actions, recent items and workspace search in
 * one keyboard-driven list. Results come from `GET /api/v1/search`, permission-filtered on the server.
 */
export function CommandPalette({ isOpen, onOpenChange }: { isOpen: boolean; onOpenChange: (o: boolean) => void }) {
  const { can, workspace } = useSession();
  useTerms();
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [failed, setFailed] = useState(false);
  const [active, setActive] = useState(0);
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const canSearch = can('policy:view') || can('sop:view') || can('member:view');

  useEffect(() => { if (isOpen) { setQ(''); setHits(null); setActive(0); setFailed(false); } }, [isOpen]);

  // Debounced search; stale responses are ignored.
  useEffect(() => {
    const term = q.trim();
    if (!term || !canSearch) { setHits(null); setSearching(false); return; }
    setSearching(true);
    let live = true;
    const t = setTimeout(() => {
      api<{ items: Hit[] }>(`/api/v1/search?q=${encodeURIComponent(term)}&types=policy,sop,member&limit=8`)
        .then((r) => { if (live) { setHits(r.items); setFailed(false); } })
        .catch(() => { if (live) { setHits([]); setFailed(true); } })
        .finally(() => { if (live) setSearching(false); });
    }, 140);
    return () => { live = false; clearTimeout(t); };
  }, [q, canSearch]);

  const options = useMemo<Option[]>(() => {
    const term = q.trim();
    const go: Option[] = navGroups(can).flatMap((g) => g.items).map((i) => ({ id: `go-${i.key}`, section: 'Go to', label: `Go to ${i.label}`, icon: i.icon, to: i.to, hint: i.keywords }));
    const make: Option[] = createActions(can).map((a) => ({ id: a.id, section: 'Create', label: a.label, icon: a.icon, to: a.to, hint: a.keywords }));
    if (!term) {
      const recent: Option[] = recentItems(workspace?.workspace_id).map((r) => ({ id: `recent-${r.to}`, section: 'Recent', label: r.title, icon: KIND[r.kind]?.icon ?? Clock, to: r.to, recent: r }));
      return [...recent, ...go, ...make];
    }
    const rank = (o: Option) => {
      const bare = score(o.label.replace(/^Go to /, ''), term);
      return Math.max(score(o.label, term), bare ? bare + 0.5 : 0, o.hint ? score(o.hint, term, false) * 0.5 : 0);
    };
    const cmds = [...go, ...make].map((o) => [o, rank(o)] as const).filter(([, s]) => s > 0).sort((a, b) => b[1] - a[1]).slice(0, 5).map(([o]) => ({ ...o, section: 'Commands' }));
    const found: Option[] = (hits ?? []).flatMap((h) => {
      const k = KIND[h.type];
      if (!k) return [];
      return [{ id: `${h.type}-${h.id}`, section: k.section, label: h.title, hint: h.type === 'member' ? h.subtitle ?? undefined : k.label, snippet: h.snippet, icon: k.icon, avatar: h.type === 'member' ? h.title : undefined, to: k.to(h), recent: { kind: h.type, id: h.id, title: h.title, to: k.to(h) } }];
    });
    return [...found, ...cmds];
  }, [q, hits, can, workspace?.workspace_id]);

  useEffect(() => { setActive(0); }, [q, hits]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const choose = (o: Option | undefined) => {
    if (!o) return;
    if (o.recent) rememberRecent(workspace?.workspace_id, o.recent);
    onOpenChange(false);
    navigate(o.to);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive((a) => Math.min(options.length - 1, a + 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((a) => Math.max(0, a - 1)); }
    else if (e.key === 'Home' && e.ctrlKey) { e.preventDefault(); setActive(0); }
    else if (e.key === 'End' && e.ctrlKey) { e.preventDefault(); setActive(options.length - 1); }
    else if (e.key === 'Enter') { e.preventDefault(); choose(options[active]); }
  };

  const sections = options.reduce<{ name: string; items: [Option, number][] }[]>((acc, o, i) => {
    const last = acc[acc.length - 1];
    if (last && last.name === o.section) last.items.push([o, i]);
    else acc.push({ name: o.section, items: [[o, i]] });
    return acc;
  }, []);
  const term = q.trim();
  const status = !term ? '' : searching ? 'Searching…' : `${options.length} ${options.length === 1 ? 'result' : 'results'}`;

  return (
    <ModalOverlay isOpen={isOpen} onOpenChange={onOpenChange} isDismissable className="cmd-overlay">
      <Modal className="cmd-modal">
        <Dialog className="cmd" aria-label="Search and commands">
          <div className="cmd-input-row">
            <Search aria-hidden="true" size="1.0625rem" className="cmd-input-icon" />
            <input
              className="cmd-input" autoFocus value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={onKeyDown}
              role="combobox" aria-expanded="true" aria-controls={listId} aria-autocomplete="list"
              aria-activedescendant={options[active] ? `${listId}-${active}` : undefined}
              aria-label="Search documents and people, or type a command"
              placeholder={canSearch ? 'Search documents and people, or type a command' : 'Type a command'}
              spellCheck={false} autoComplete="off"
            />
            <kbd className="cmd-esc">Esc</kbd>
          </div>
          <div className="cmd-list" id={listId} role="listbox" aria-label="Results" ref={listRef}>
            {sections.map((s, si) => (
              <div key={s.name} role="group" aria-labelledby={`${listId}-g${si}`} className="cmd-group">
                <div className="cmd-group-label" id={`${listId}-g${si}`} role="presentation">{s.name}</div>
                {s.items.map(([o, i]) => {
                  const Icon = o.icon ?? ArrowRight;
                  return (
                    <div
                      key={o.id} id={`${listId}-${i}`} data-index={i} role="option" aria-selected={i === active}
                      className="cmd-option" onMouseMove={() => i !== active && setActive(i)} onClick={() => choose(o)}
                    >
                      {o.avatar ? <Avatar name={o.avatar} size="1.375rem" /> : <span className="cmd-option-icon"><Icon aria-hidden="true" size="0.9375rem" /></span>}
                      <span className="cmd-option-text">
                        <span className="cmd-option-label">{o.label}</span>
                        {o.snippet ? <Snippet html={o.snippet} /> : null}
                      </span>
                      {o.section !== 'Go to' && o.section !== 'Commands' && o.section !== 'Create' && o.hint && <span className="cmd-option-hint">{o.hint}</span>}
                      {i === active && <CornerDownLeft aria-hidden="true" size="0.875rem" className="cmd-enter" />}
                    </div>
                  );
                })}
              </div>
            ))}
            {term && !searching && options.length === 0 && (
              <div className="cmd-empty">
                <p className="cmd-empty-title">{failed ? 'Search isn’t available right now' : `Nothing matches “${term}”`}</p>
                <p>{failed ? 'Try again in a moment. Commands still work.' : 'Try a shorter word or a person’s name.'}</p>
              </div>
            )}
          </div>
          <div className="cmd-foot" aria-hidden="true">
            <span><kbd>↑</kbd><kbd>↓</kbd> to move</span><span><kbd>↵</kbd> to open</span><span><kbd>Esc</kbd> to close</span>
          </div>
          <p className="visually-hidden" role="status" aria-live="polite">{status}</p>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
