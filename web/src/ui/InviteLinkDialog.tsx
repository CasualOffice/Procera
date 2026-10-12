import { Check, Copy } from 'lucide-react';
import { useState } from 'react';
import { Button } from './components/Button';
import { FormDialog } from './components/FormDialog';
import { TextField } from './components/TextField';

/**
 * Shows a freshly issued invitation link for the admin to hand over themselves, for when email
 * isn't set up (docs/08 "Members", "System API"). Issuing it already made any earlier link stop working.
 */
export function InviteLinkDialog({ email, link, onClose }: { email: string; link: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try { await navigator.clipboard?.writeText(link); setCopied(true); } catch { setCopied(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Invitation link" width={560}
      description={`Anyone with this link can join as ${email}. It works once and expires in 7 days.`}
      footer={<Button variant="secondary" onPress={onClose}>Done</Button>}>
      <div style={{ display: 'flex', gap: 'var(--space-2)', alignItems: 'flex-end' }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <TextField label="Invitation link" value={link} isReadOnly onFocus={(e) => (e.target as HTMLInputElement).select()} />
        </div>
        <Button onPress={() => void copy()}>{copied ? <Check aria-hidden="true" size="1rem" /> : <Copy aria-hidden="true" size="1rem" />} {copied ? 'Copied' : 'Copy link'}</Button>
      </div>
      <p className="muted" role="status" style={{ margin: 0 }}>
        {copied ? 'Link copied. ' : ''}Any earlier link for this invitation no longer works. Share it only with {email}.
      </p>
    </FormDialog>
  );
}
