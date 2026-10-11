import { ArrowRight, BookOpen, ClipboardCheck, FileText, LayoutTemplate, ScrollText } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useItems } from '../lib/data';
import { Link, navigate } from '../lib/router';
import { term, useTerms } from '../lib/terms';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { KIND_TAG, STATE_TAG, Tag } from '../ui/Tag';

interface PackDoc { key: string; kind: string; title: string; summary: string; existing: { artifact_id: string; state: string; link: string } | null }
interface Suggestion { document_key: string; title: string; audience: string; due_in_days: number }
export interface Pack { key: string; version: string; name: string; description: string; documents: PackDoc[]; campaign: Suggestion | null; can_install: boolean }
interface Installed { template_document: string; outcome: 'created' | 'copied' | 'skipped'; kind: string; key: string; title: string; artifact_id: string; state: string; link: string }
interface Report { template: string; version: string; documents: Installed[]; campaign: { title: string; due_in_days: number; artifact_id: string; document_title: string; status: 'publish_first' | 'ready'; link: string } | null }

const kindIcon = (kind: string, size = '0.875rem') => (kind === 'sop' ? <ScrollText aria-hidden="true" size={size} /> : <FileText aria-hidden="true" size={size} />);
const kindLabel = (kind: string) => (kind === 'sop' ? term('sop', { capitalised: true }) : KIND_TAG[kind]?.label ?? kind);

/** Templates (docs/05 §3.7, BPMS-106 P0 subset): built-in packs of draft documents to adapt. */
export function Templates() {
  useTerms(); // the workspace's terms (CFG-008)
  const [packs, reload, error] = useItems<Pack>('/api/v1/templates');
  const [installing, setInstalling] = useState<Pack | null>(null);
  const anyInstallable = (packs ?? []).some((p) => p.can_install);

  return (
    <>
      <div className="page-head">
        <div><h1>Templates</h1><p>Start from reviewed starter documents instead of a blank page. Everything installs as a draft for you to adapt, review and publish.</p></div>
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="templates" /> : packs === null ? <Skeleton rows={2} label="Loading templates" /> : packs.length === 0 ? (
        <Empty art="templates" title="No templates available">Templates appear here when your Procera version includes them.</Empty>
      ) : (
        <>
          {!anyInstallable && <Notice tone="info" title="You can look, but not install">Ask a workspace admin to install a template for your workspace.</Notice>}
          <ul className="packs" aria-label="Template packs">
            {packs.map((p) => {
              const installed = p.documents.filter((d) => d.existing).length;
              return (
                <li key={p.key} className="pack" aria-labelledby={`pack-${p.key}`}>
                  <div className="pack-head">
                    <span className="pack-icon tag-purple"><LayoutTemplate aria-hidden="true" size="1rem" /></span>
                    <div className="pack-title">
                      <h2 id={`pack-${p.key}`}>{p.name}</h2>
                      <p>{p.description}</p>
                    </div>
                    {p.can_install && <Button className="pack-install" aria-label={`Install ${p.name}`} onPress={() => setInstalling(p)}>Install</Button>}
                  </div>
                  <div className="pack-body">
                    <p className="pack-label">Creates {p.documents.length} draft {p.documents.length === 1 ? 'document' : 'documents'}{installed ? ` · ${installed} already in your library` : ''}</p>
                    <ul className="pack-docs">
                      {p.documents.map((d) => (
                        <li key={d.key}>
                          <span className={`doc-icon doc-icon-sm tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{kindIcon(d.kind, '0.8125rem')}</span>
                          <span className="pack-doc-main"><strong>{d.title}</strong><small>{d.summary}</small></span>
                          <span className="pack-doc-kind">{kindLabel(d.kind)}</span>
                          {d.existing && <Tag color="green">In your library</Tag>}
                        </li>
                      ))}
                    </ul>
                    {p.campaign && (
                      <p className="pack-campaign"><ClipboardCheck aria-hidden="true" size="0.9375rem" /> Suggests an acknowledgement campaign for {p.documents.find((d) => d.key === p.campaign?.document_key)?.title ?? 'one document'}: everyone, due {p.campaign.due_in_days} days after launch.</p>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        </>
      )}
      <InstallWizard pack={installing} onClose={() => setInstalling(null)} onInstalled={reload} />
    </>
  );
}

/** Two short steps (documents, then acknowledgement and review), ending on what was created. */
function InstallWizard({ pack, onClose, onInstalled }: { pack: Pack | null; onClose: () => void; onInstalled: () => void }) {
  useTerms();
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [campaign, setCampaign] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  useEffect(() => {
    if (!pack) return;
    // Documents already in the library start unticked: ticking one creates a numbered copy.
    setChosen(new Set(pack.documents.filter((d) => !d.existing).map((d) => d.key)));
    setCampaign(!!pack.campaign); setStep(1); setError(null); setReport(null); setBusy(false);
  }, [pack]);
  const docs = useMemo(() => (pack?.documents ?? []).filter((d) => chosen.has(d.key)), [pack, chosen]);
  const copies = docs.filter((d) => d.existing);
  const target = pack?.campaign ? pack.documents.find((d) => d.key === pack.campaign?.document_key) : undefined;
  // The plan follows the target document when this install creates it or it already exists.
  const canPlan = !!target && (chosen.has(target.key) || !!target.existing);
  const toggle = (key: string, on: boolean) => setChosen((s) => { const n = new Set(s); if (on) n.add(key); else n.delete(key); return n; });

  const install = async () => {
    if (!pack) return;
    setBusy(true); setError(null);
    try {
      // The target is sent too when it already exists, so the plan can point at it (it is skipped, not copied).
      const keys = [...chosen];
      if (campaign && canPlan && target && !chosen.has(target.key)) keys.push(target.key);
      const r = await api<Report>(`/api/v1/templates/${pack.key}/install`, { method: 'POST', body: { documents: keys, campaign: campaign && canPlan, on_conflict: copies.length ? 'copy' : 'skip' } });
      setReport(r); setStep(3); onInstalled();
    } catch (e) { setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not install the template. Try again.'); } finally { setBusy(false); }
  };

  if (!pack) return null;
  const title = step === 3 ? 'Template installed' : `Install ${pack.name}`;
  const description = step === 1 ? 'Step 1 of 2 · Choose the documents to create as drafts.' : step === 2 ? 'Step 2 of 2 · Check what happens next.' : 'Adapt each draft, then publish it from the library.';
  const footer = step === 1 ? (
    <><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => setStep(2)} isDisabled={chosen.size === 0}>Continue</Button></>
  ) : step === 2 ? (
    <><Button variant="secondary" onPress={() => setStep(1)} isDisabled={busy}>Back</Button><Button onPress={() => void install()} isDisabled={busy}>{busy ? 'Installing…' : `Install ${docs.length} ${docs.length === 1 ? 'document' : 'documents'}`}</Button></>
  ) : (
    <><Button variant="secondary" onPress={() => { onClose(); navigate('/library'); }}>Open library</Button><Button onPress={onClose}>Done</Button></>
  );

  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} width={600} title={title} description={description} footer={footer}>
      {error && <Notice tone="danger" title={error} />}
      {step === 1 && (
        <div className="field" role="group" aria-labelledby="tpl-docs-label">
          <span className="field-label" id="tpl-docs-label">Documents</span>
          <ul className="wizard-docs">
            {pack.documents.map((d) => (
              <li key={d.key}>
                <Checkbox isSelected={chosen.has(d.key)} onChange={(on) => toggle(d.key, on)}>{d.title}</Checkbox>
                <span className="wizard-doc-meta">{kindLabel(d.kind)}{d.existing ? (chosen.has(d.key) ? ' · already in your library, a numbered copy will be created' : ' · already in your library') : ''}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {step === 2 && (
        <>
          <p className="wizard-summary">Creates {docs.length} {docs.length === 1 ? 'draft' : 'drafts'} owned by you: {docs.map((d) => d.title).join(', ')}.{copies.length ? ` ${copies.length} of them ${copies.length === 1 ? 'is a copy' : 'are copies'} because the original is already in your library.` : ''} Nobody else sees a draft until it is published.</p>
          {target && (canPlan ? (
            <div className="wizard-campaign">
              <Checkbox isSelected={campaign} onChange={setCampaign}>Plan an acknowledgement campaign for {target.title}</Checkbox>
              <small>Everyone, due {pack.campaign?.due_in_days} days after launch. You start it from Campaigns once {target.title} is published.</small>
            </div>
          ) : <p className="muted">The acknowledgement campaign needs {target.title}. Go back and tick it to plan the campaign.</p>)}
        </>
      )}
      {step === 3 && report && (
        <>
          <ul className="result-list" aria-label="Installed documents">
            {report.documents.map((d) => (
              <li key={d.artifact_id + d.outcome}>
                <span className={`doc-icon doc-icon-sm tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{kindIcon(d.kind, '0.8125rem')}</span>
                <Link to={d.link}>{d.title}</Link>
                {d.outcome === 'skipped'
                  ? <Tag color="gray">Already in your library · {STATE_TAG[d.state]?.label ?? d.state}</Tag>
                  : <Tag color={d.outcome === 'copied' ? 'yellow' : 'green'} dot>{d.outcome === 'copied' ? 'Copy created' : 'Draft created'}</Tag>}
              </li>
            ))}
          </ul>
          {report.campaign && (
            <div className="result-next">
              <p><strong>Next:</strong> {report.campaign.status === 'ready' ? `${report.campaign.document_title} is published. Start the campaign to ask everyone to acknowledge it.` : `review and publish ${report.campaign.document_title}, then start the acknowledgement campaign. It asks everyone, due ${report.campaign.due_in_days} days after launch.`}</p>
              <Link className="btn btn-secondary btn-md link-btn" to={report.campaign.link}><ClipboardCheck aria-hidden="true" size="1rem" /> Set up the acknowledgement campaign</Link>
            </div>
          )}
          <p className="muted result-foot"><BookOpen aria-hidden="true" size="0.875rem" /> Drafts are in the <Link to="/library">library <ArrowRight aria-hidden="true" size="0.75rem" /></Link></p>
        </>
      )}
    </FormDialog>
  );
}
