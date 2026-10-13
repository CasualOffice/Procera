import { BookCheck, CheckSquare, ChevronDown, CircleCheck, Stamp } from 'lucide-react';
import { Button, ComboBox, FieldError, Input, Label, ListBox, ListBoxItem, Popover, Text, TextArea, TextField as AriaTextField } from 'react-aria-components';
import type { StepKind } from './model';
import type { TagColor } from '../../ui/Tag';

/* Small pieces the studio's three panels share. */

export const KIND_LOOK: Record<StepKind, { label: string; color: TagColor; Icon: typeof Stamp }> = {
  approval: { label: 'Approval', color: 'purple', Icon: Stamp },
  task: { label: 'Task', color: 'blue', Icon: CheckSquare },
  acknowledge: { label: 'Acknowledgement', color: 'green', Icon: BookCheck },
};
export const EndIcon = CircleCheck;

/** A text field that suggests known values but accepts any (a group by name, a person's email). */
export function Suggest({ label, value, onChange, items, description, errorMessage, placeholder }: {
  label: string; value: string; onChange: (v: string) => void; items: { id: string; label: string; detail?: string }[];
  description?: string; errorMessage?: string; placeholder?: string;
}) {
  return (
    <ComboBox className="field studio-suggest" inputValue={value} onInputChange={onChange} allowsCustomValue menuTrigger="focus"
      onSelectionChange={(k) => { const hit = items.find((i) => i.id === k); if (hit) onChange(hit.id); }} isInvalid={!!errorMessage}>
      <Label className="field-label">{label}</Label>
      <div className="studio-suggest-box">
        <Input className="field-input" placeholder={placeholder} />
        <Button className="studio-suggest-btn" aria-label={`Show ${label.toLowerCase()} suggestions`}><ChevronDown aria-hidden="true" size="0.875rem" /></Button>
      </div>
      {description && <Text slot="description" className="field-description">{description}</Text>}
      <FieldError className="field-error">{errorMessage}</FieldError>
      <Popover className="popover">
        <ListBox className="listbox" items={items} renderEmptyState={() => <p className="studio-suggest-empty">No matches. You can still type a name.</p>}>
          {(i) => <ListBoxItem id={i.id} textValue={i.label} className="listbox-item">{i.label}{i.detail && <span className="studio-suggest-detail">{i.detail}</span>}</ListBoxItem>}
        </ListBox>
      </Popover>
    </ComboBox>
  );
}

/** A multi-line text field in the app's form-control style. */
export function LongText({ label, value, onChange, description, rows = 3, isReadOnly }: { label: string; value: string; onChange: (v: string) => void; description?: string; rows?: number; isReadOnly?: boolean }) {
  return (
    <AriaTextField className="field" value={value} onChange={onChange} isReadOnly={isReadOnly}>
      <Label className="field-label">{label}</Label>
      <TextArea className="field-input req-textarea" rows={rows} />
      {description && <Text slot="description" className="field-description">{description}</Text>}
    </AriaTextField>
  );
}

/** Whole days from a text box: null when empty or not a number from `min` to `max`. */
export function daysValue(text: string, min = 1, max = 60): number | null {
  if (!/^\d{1,3}$/.test(text.trim())) return null;
  const n = Number(text.trim());
  return n >= min && n <= max ? n : null;
}
