import { useState } from 'react';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Select } from '../ui/components/Select';
import { ConfirmDialog } from '../ui/components/Dialog';
import { Tabs } from '../ui/components/Tabs';
import { DataGrid, type GridColumn } from '../ui/components/DataGrid';

interface RequestRow { id: string; title: string; process: string; assignee: string; due: string; status: string }
const ROWS: RequestRow[] = [
  { id: 'PR-2026-000123', title: 'Laptops for design team', process: 'Purchase requisition', assignee: 'R. Iyer', due: '2026-10-14', status: 'Awaiting approval' },
  { id: 'HR-2026-000045', title: 'Onboard A. Mehta', process: 'Employee onboarding', assignee: 'IT service desk', due: '2026-10-20', status: 'In progress' },
  { id: 'IT-2026-000310', title: 'Prod DB read access', process: 'Access request', assignee: 'Security', due: '2026-10-12', status: 'Overdue' },
  { id: 'PR-2026-000124', title: 'Conference travel', process: 'Purchase requisition', assignee: 'Finance approvers', due: '2026-10-18', status: 'Awaiting approval' },
  { id: 'QA-2026-000007', title: 'Deviation in batch 42', process: 'CAPA', assignee: 'Quality', due: '2026-11-02', status: 'Root-cause analysis' },
];
const COLUMNS: GridColumn<RequestRow>[] = [
  { id: 'id', header: 'Request', value: (r) => r.id },
  { id: 'title', header: 'Title', value: (r) => r.title },
  { id: 'process', header: 'Process', value: (r) => r.process, type: 'enum' },
  { id: 'assignee', header: 'Current owner', value: (r) => r.assignee },
  { id: 'due', header: 'Due', value: (r) => r.due, type: 'date' },
  { id: 'status', header: 'Status', value: (r) => r.status, type: 'enum' },
];
import tokens from '../ui/tokens.json';
import { Empty } from '../ui/States';
import { ART_KINDS } from '../ui/illustrations';

/** Design-system gallery: the living reference for BPMS-010 and the target of E2E accessibility scans. */
export function DesignGallery() {
  const [ack, setAck] = useState(false);
  const [deleted, setDeleted] = useState(false);
  const [replay, setReplay] = useState(0);
  return (
    <article>
      <h1>Design system</h1>
      <p>Foundations for Procera's interface: calm, editorial, accessible. Every colour pairing below is checked against WCAG 2.2 AA in CI.</p>

      <section aria-labelledby="buttons">
        <h2 id="buttons">Buttons</h2>
        <div className="row">
          <Button variant="primary">Approve</Button>
          <Button variant="secondary">Reject</Button>
          <Button variant="danger">Delete policy</Button>
          <Button variant="ghost">View history</Button>
          <Button variant="accent">Publish</Button>
          <Button isDisabled>Unavailable</Button>
        </div>
      </section>

      <section aria-labelledby="fields">
        <h2 id="fields">Fields</h2>
        <p className="muted">One control system: 2.25rem tall, a quiet border that darkens on hover, an ink border and soft ring on focus, errors as a red ring (nothing shifts).</p>
        <div className="gallery-form">
          <TextField label="Request title" description="Shown to approvers in their inbox." isRequired placeholder="For example “Laptops for design team”" />
          <TextField label="Cost centre" defaultValue="ENG-01" />
          <TextField label="Amount" isRequired isInvalid errorMessage="Enter an amount above zero." defaultValue="-20" />
          <TextField label="Request ID" defaultValue="PR-2026-000123" isReadOnly description="Assigned when you send it." />
          <TextField label="Approver" defaultValue="Finance approvers" isDisabled />
          <Select label="Department" description="Used to route approvals." placeholder="Choose a department" options={[{ id: 'fin', label: 'Finance' }, { id: 'hr', label: 'Human resources' }, { id: 'it', label: 'IT' }, { id: 'ops', label: 'Operations' }]} />
          <Select label="Priority" isRequired isInvalid errorMessage="Choose a priority." options={[{ id: 'low', label: 'Low' }, { id: 'high', label: 'High' }]} />
          <div className="field">
            <label className="field-label" htmlFor="g-native">Time range (native select)</label>
            <select id="g-native" defaultValue="30"><option value="7">Last 7 days</option><option value="30">Last 30 days</option><option value="90">Last 90 days</option></select>
          </div>
          <div className="field">
            <label className="field-label" htmlFor="g-date">Start date</label>
            <input id="g-date" type="date" defaultValue="2026-12-21" />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="g-dt">Go live</label>
            <input id="g-dt" type="datetime-local" defaultValue="2026-11-02T09:00" />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="g-num">Reminder after (days)</label>
            <input id="g-num" type="number" defaultValue={2} min={0} />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="g-search">Find a request</label>
            <input id="g-search" type="search" placeholder="Search requests" />
          </div>
          <div className="field gallery-wide">
            <label className="field-label" htmlFor="g-text">Justification</label>
            <textarea id="g-text" placeholder="Why is this needed now?" />
            <span className="field-description">Approvers see this first.</span>
          </div>
          <div className="field">
            <span className="field-label">Brand colour</span>
            <span className="row"><input type="color" defaultValue="#EE4136" aria-label="Brand colour" /><input defaultValue="#EE4136" aria-label="Hex value" style={{ width: '7rem' }} /></span>
          </div>
          <fieldset className="field gallery-choices">
            <legend className="field-label">Choices</legend>
            <Checkbox isSelected={ack} onChange={setAck}>I have read and understood the Code of Conduct</Checkbox>
            <label className="check-row"><input type="checkbox" defaultChecked /> Email me a copy</label>
            <label className="check-row"><input type="checkbox" ref={(el) => { if (el) el.indeterminate = true; }} /> Some selected</label>
            <label className="check-row"><input type="checkbox" disabled /> Not available</label>
            <label className="check-row"><input type="radio" name="g-r" defaultChecked /> Notify now</label>
            <label className="check-row"><input type="radio" name="g-r" /> Notify in the daily digest</label>
          </fieldset>
        </div>
      </section>

      <section aria-labelledby="dialogs">
        <h2 id="dialogs">Dialogs</h2>
        <div className="row">
          <ConfirmDialog trigger={<Button variant="danger">Delete policy</Button>} title="Delete “Travel policy v3”?" confirmLabel="Delete policy" destructive onConfirm={() => setDeleted(true)}>
            <p>The draft and its 4 comments will be removed. Published versions stay in the audit trail.</p>
          </ConfirmDialog>
          {deleted && <span role="status">Policy deleted</span>}
        </div>
      </section>

      <section aria-labelledby="tabs-h">
        <h2 id="tabs-h">Tabs</h2>
        <Tabs label="Process detail" tabs={[
          { id: 'overview', label: 'Overview', content: <p>Owner, purpose, KPIs and linked policies.</p> },
          { id: 'sop', label: 'SOP', content: <p>The governed procedure, version 7.</p> },
          { id: 'versions', label: 'Versions', content: <p>Version history with semantic diffs.</p> },
        ]} />
      </section>

      <section aria-labelledby="grid-h">
        <h2 id="grid-h">Data grid</h2>
        <DataGrid caption="All requests" data={ROWS} columns={COLUMNS} selectable />
      </section>

      <section aria-labelledby="notices">
        <h2 id="notices">Notices</h2>
        <div className="stack">
          <Notice tone="info" title="Policy review due in 14 days">The owner has been reminded.</Notice>
          <Notice tone="success" title="Request PR-2026-000123 approved" />
          <Notice tone="warning" title="3 approvals are overdue in Finance" />
          <Notice tone="danger" title="The file failed the malware scan and was quarantined" />
        </div>
      </section>

      <section aria-labelledby="empty-h">
        <h2 id="empty-h">Empty states</h2>
        <p>Each kind of emptiness has its drawing. It plays once on arrival and holds still under reduced motion.</p>
        <Button variant="secondary" onPress={() => setReplay((n) => n + 1)}>Replay</Button>
        <ul className="art-gallery" key={replay}>
          {ART_KINDS.map((k) => <li key={k}><Empty art={k} title={k}>Illustration for the {k} empty state.</Empty></li>)}
          <li><Empty compact art="no-match" title="Compact" action={<Button variant="secondary">Clear filters</Button>}>Inside drawers and cards.</Empty></li>
        </ul>
      </section>

      <section aria-labelledby="type">
        <h2 id="type">Type scale</h2>
        <ul className="plain">
          {Object.entries(tokens.font.size).map(([k, v]) => (
            <li key={k} style={{ fontSize: v }}>{k} · {v}: Turn processes into progress.</li>
          ))}
        </ul>
      </section>
    </article>
  );
}
