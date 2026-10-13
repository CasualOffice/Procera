import { Check, Copy, KeyRound, Plus, Trash2 } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { actionLabel, groupByArea, permLabel } from '../lib/roles';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { ConfirmDialog } from '../ui/components/Dialog';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { StepUpDialog } from '../ui/StepUp';
import { Tag } from '../ui/Tag';

interface Token {
  id: string; name: string; last4: string; permissions: string[]; created_at: string; expires_at: string;
  last_used_at: string | null; last_used_ip: string | null; revoked_at: string | null; state: 'active' | 'expired' | 'revoked';
  owner: { id: string; name: string };
}

const LIFETIMES = [7, 30, 90, 180, 365].map((d) => ({ id: String(d), label: d === 365 ? '1 year' : `${d} days` }));
const STATE: Record<Token['state'], { label: string; color: 'green' | 'gray' | 'red' }> = {
  active: { label: 'Active', color: 'green' }, expired: { label: 'Expired', color: 'gray' }, revoked: { label: 'Revoked', color: 'red' },
};
const isRead = (p: string) => /:view/.test(p);
/** Areas a script most often needs come first; tokens can't manage tokens, so that area is left out. */
const AREA_ORDER = ['request', 'work', 'sop', 'policy', 'announcement', 'campaign', 'process', 'document', 'member', 'directory', 'audit', 'file'];
const rank = (p: string) => { const i = AREA_ORDER.indexOf(p.slice(0, p.indexOf(':'))); return i < 0 ? AREA_ORDER.length : i; };
const errorText = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ? `${e.problem.title}: ${permLabel(e.problem.detail)}` : e.problem.title) : fallback);

/**
 * Account › API tokens (BPMS-144, docs/06 §3.5): tokens that let a script use Procera as you, with only
 * the permissions you pick from your own, for up to a year. The secret is shown once. People who manage
 * API tokens also see and can revoke everyone's.
 */
export function ApiTokens() {
  const { permissions, can, membershipId } = useSession();
  const [items, refresh, error] = useItems<Token>('/api/v1/me/api-tokens');
  const admin = can('api_token:manage');
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<{ name: string; token: string } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [rev, setRev] = useState(0); // remounts everyone's list when tokens change here

  const revoke = async (t: Token, path: string) => {
    setListError(null);
    try {
      await api(`${path}/${t.id}`, { method: 'DELETE' });
      setNotice(`“${t.name}” was revoked. Anything using it stops working now.`);
      refresh(); setRev((r) => r + 1);
    } catch (e) { setListError(errorText(e, 'Couldn’t revoke the token. Try again.')); }
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>API tokens</h1>
          <p>Let scripts and other tools use Procera as you, in this workspace. A token can do only what you allow, and never approves, acknowledges or signs.</p>
        </div>
        <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="0.9375rem" /> New token</Button>
      </div>
      {notice && <Notice tone="success" title={notice} />}
      {listError && <Notice tone="danger" title={listError} />}

      <section className="account-section" aria-labelledby="tok-mine">
        <div className="section-head"><h2 id="tok-mine">Your tokens</h2>{items && items.length > 0 && <span className="account-count">{items.length}</span>}</div>
        {items === null ? <Skeleton rows={2} label="Loading your tokens" />
          : error ? <LoadError error={error} onRetry={refresh} what="your tokens" />
            : items.length === 0 ? (
              <div className="card"><Empty compact icon={KeyRound} title="No tokens yet">Make one when a script or another tool needs to read or start things in Procera.</Empty></div>
            ) : <TokenRows items={items} label="Your tokens" onRevoke={(t) => void revoke(t, '/api/v1/me/api-tokens')} />}
      </section>

      {admin && <EveryonesTokens key={rev} me={membershipId} onRevoke={(t) => void revoke(t, '/api/v1/api-tokens')} />}

      {creating && (
        <CreateToken held={permissions} onClose={() => setCreating(false)}
          onCreated={(name, token) => { setCreating(false); setCreated({ name, token }); refresh(); setRev((r) => r + 1); }} />
      )}
      {created && <ShowOnce name={created.name} token={created.token} onClose={() => { setNotice(`“${created.name}” is ready.`); setCreated(null); }} />}
    </>
  );
}

function TokenRows({ items, label, onRevoke, showOwner = false }: { items: Token[]; label: string; onRevoke: (t: Token) => void; showOwner?: boolean }) {
  return (
    <ul className="card rows" aria-label={label}>
      {items.map((t) => (
        <li key={t.id}>
          <span className="row-icon"><KeyRound aria-hidden="true" size="1rem" /></span>
          <div className="row-main">
            <p className="row-title">{t.name} <code className="token-last4">…{t.last4}</code> <Tag color={STATE[t.state].color} dot>{STATE[t.state].label}</Tag></p>
            <p className="row-sub">
              {showOwner && <>{t.owner.name} · </>}
              {t.state === 'active' ? `Expires ${shortDate(t.expires_at)}` : t.state === 'expired' ? `Expired ${shortDate(t.expires_at)}` : `Revoked ${shortDate(t.revoked_at!)}`}
              {' · '}{t.last_used_at ? `Last used ${timeAgo(t.last_used_at)}${t.last_used_ip ? ` from ${t.last_used_ip}` : ''}` : 'Not used yet'}
            </p>
            <p className="row-sub token-perms" title={t.permissions.map(permLabel).join(', ')}>{t.permissions.length === 1 ? permLabel(t.permissions[0]!) : `${t.permissions.length} permissions: ${t.permissions.slice(0, 3).map(permLabel).join(', ')}${t.permissions.length > 3 ? '…' : ''}`}</p>
          </div>
          {t.state === 'active' && (
            <ConfirmDialog destructive title={`Revoke “${t.name}”?`} confirmLabel="Revoke token" onConfirm={() => onRevoke(t)}
              trigger={<Button variant="ghost" className="btn-sm" aria-label={`Revoke ${t.name}`}><Trash2 aria-hidden="true" size="0.875rem" /> Revoke</Button>}>
              <p>Anything using this token stops working straight away. This can’t be undone.</p>
            </ConfirmDialog>
          )}
        </li>
      ))}
    </ul>
  );
}

function EveryonesTokens({ me, onRevoke }: { me: string | null; onRevoke: (t: Token) => void }) {
  const [items, refresh, error] = useItems<Token>('/api/v1/api-tokens');
  const active = items?.filter((t) => t.state === 'active' && t.owner.id !== me) ?? [];
  return (
    <section className="account-section" aria-labelledby="tok-all">
      <div className="section-head"><h2 id="tok-all">Other people’s active tokens</h2>{active.length > 0 && <span className="account-count">{active.length}</span>}</div>
      <p className="account-lede">You manage API tokens in this workspace, so you can revoke anyone’s.</p>
      {items === null ? <Skeleton rows={2} label="Loading tokens" />
        : error ? <LoadError error={error} onRetry={refresh} what="tokens" />
          : active.length === 0 ? <div className="card"><Empty compact icon={KeyRound} title="No other active tokens">Nobody else in this workspace has an active API token.</Empty></div>
            : <TokenRows items={active} label="Other people’s active tokens" showOwner onRevoke={onRevoke} />}
    </section>
  );
}

function CreateToken({ held, onClose, onCreated }: { held: Set<string>; onClose: () => void; onCreated: (name: string, token: string) => void }) {
  const mine = useMemo(() => [...held].filter((p) => p.includes(':') && !p.startsWith('api_token:')).sort((a, b) => rank(a) - rank(b) || a.localeCompare(b)), [held]);
  const groups = useMemo(() => groupByArea(mine), [mine]);
  const [name, setName] = useState('');
  const [days, setDays] = useState('90');
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [stepUp, setStepUp] = useState(false);
  const toggle = (p: string, on: boolean) => setPicked((s) => { const n = new Set(s); if (on) n.add(p); else n.delete(p); return n; });

  const submit = async () => {
    setBusy(true); setErr(null);
    try {
      const r = await api<{ token: string; name: string }>('/api/v1/me/api-tokens', { method: 'POST', body: { name: name.trim(), permissions: [...picked], expires_in_days: Number(days) } });
      onCreated(r.name, r.token);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else setErr(errorText(e, 'Couldn’t make the token. Try again.'));
    } finally { setBusy(false); }
  };

  return (
    <>
      <FormDialog isOpen={!stepUp} onOpenChange={(o) => !o && !busy && onClose()} title="New API token" width={600}
        description="Pick only what the script needs. You can revoke the token at any time."
        footer={<><Button variant="secondary" onPress={onClose} isDisabled={busy}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy || !name.trim() || picked.size === 0}>{busy ? 'Making…' : 'Make token'}</Button></>}>
        {err && <Notice tone="danger" title={err} />}
        <Form onSubmit={(e) => { e.preventDefault(); void submit(); }} className="stack">
          <div className="token-head">
            <TextField label="Name" value={name} onChange={setName} maxLength={100} autoFocus placeholder="e.g. Nightly report" description="So you know what uses it." />
            <Select label="Expires after" options={LIFETIMES} selectedKey={days} onSelectionChange={(k) => k && setDays(String(k))} />
          </div>
          <fieldset className="token-perms-pick">
            <legend>What it may do <span className="muted">({picked.size} chosen)</span></legend>
            <div className="token-presets">
              <Button variant="secondary" className="btn-sm" onPress={() => setPicked(new Set(mine.filter(isRead)))}>Read only</Button>
              <Button variant="ghost" className="btn-sm" onPress={() => setPicked(new Set())} isDisabled={picked.size === 0}>Clear</Button>
            </div>
            <div className="token-groups">
              {groups.map((g) => (
                <div key={g.area} className="token-group" role="group" aria-label={g.label}>
                  <p className="token-group-name">{g.label}</p>
                  {g.perms.map((p) => <Checkbox key={p} isSelected={picked.has(p)} onChange={(on) => toggle(p, on)}>{actionLabel(p)}</Checkbox>)}
                </div>
              ))}
            </div>
            <p className="muted token-note">You can only give a token permissions you have. It always acts as you, so if you lose a permission, so does the token.</p>
          </fieldset>
        </Form>
      </FormDialog>
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void submit(); }} />
    </>
  );
}

function ShowOnce({ name, token, onClose }: { name: string; token: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  return (
    <FormDialog isOpen onOpenChange={() => undefined} title="Copy your token" width={560}
      description={`This is the only time you’ll see “${name}”. Store it somewhere safe, like a password manager or your tool’s secret settings.`}
      footer={<Button onPress={onClose}>I’ve copied it</Button>}>
      <div className="token-once">
        <code aria-label="Your new token">{token}</code>
        <Button variant="secondary" onPress={() => { void navigator.clipboard?.writeText(token); setCopied(true); }}>
          {copied ? <><Check aria-hidden="true" size="0.9375rem" /> Copied</> : <><Copy aria-hidden="true" size="0.9375rem" /> Copy</>}
        </Button>
      </div>
      <p className="muted">Send it as <code>Authorization: Bearer &lt;token&gt;</code>.</p>
    </FormDialog>
  );
}
