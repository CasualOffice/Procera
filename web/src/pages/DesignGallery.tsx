import { useState } from 'react';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import tokens from '../ui/tokens.json';

/** Design-system gallery: the living reference for BPMS-010 and the target of E2E accessibility scans. */
export function DesignGallery() {
  const [ack, setAck] = useState(false);
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
        <TextField label="Request title" description="Shown to approvers in their inbox." isRequired />
        <TextField label="Cost centre" defaultValue="ENG-01" />
        <Checkbox isSelected={ack} onChange={setAck}>I have read and understood the Code of Conduct</Checkbox>
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

      <section aria-labelledby="type">
        <h2 id="type">Type scale</h2>
        <ul className="plain">
          {Object.entries(tokens.font.size).map(([k, v]) => (
            <li key={k} style={{ fontSize: v }}>{k} · {v} — Turn processes into progress.</li>
          ))}
        </ul>
      </section>
    </article>
  );
}
