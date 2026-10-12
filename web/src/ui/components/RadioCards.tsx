import { FieldError, Label, Radio, RadioGroup, type RadioGroupProps } from 'react-aria-components';
import './components.css';

export interface RadioCard { id: string; label: string; meta?: string; description?: string }

/** A few exclusive choices side by side, each with a line saying what it does (React Aria radio group). */
export function RadioCards({ label, options, ...rest }: Omit<RadioGroupProps, 'children'> & { label: string; options: RadioCard[] }) {
  return (
    <RadioGroup {...rest} className="radio-cards">
      <Label className="field-label">{label}{rest.isRequired && <span aria-hidden="true" className="field-required"> *</span>}</Label>
      <div className="radio-cards-row">
        {options.map((o) => (
          <Radio key={o.id} value={o.id} className="radio-card">
            <span className="radio-card-head"><span className="radio-card-dot" aria-hidden="true" /><span className="radio-card-label">{o.label}</span>{o.meta && <span className="radio-card-meta">{o.meta}</span>}</span>
            {o.description && <span className="radio-card-desc">{o.description}</span>}
          </Radio>
        ))}
      </div>
      <FieldError className="field-error" />
    </RadioGroup>
  );
}
