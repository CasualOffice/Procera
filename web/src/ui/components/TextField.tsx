import { Eye, EyeOff } from 'lucide-react';
import { useState } from 'react';
import { Button, FieldError, Input, Label, Text, TextField as AriaTextField, type TextFieldProps as AriaTextFieldProps } from 'react-aria-components';
import './components.css';

export interface TextFieldProps extends AriaTextFieldProps {
  label: string;
  description?: string;
  errorMessage?: string;
  /** A hint inside the box (never a substitute for the label). */
  placeholder?: string;
}

/**
 * Labelled text input with description and error wired through ARIA (never placeholder-as-label).
 * Password fields get a show/hide toggle (a pressed-state button after the input, so the tab order
 * stays input → toggle and screen readers hear "Show password").
 */
export function TextField({ label, description, errorMessage, placeholder, type, ...rest }: TextFieldProps) {
  const [shown, setShown] = useState(false);
  const password = type === 'password';
  return (
    <AriaTextField {...rest} type={password && shown ? 'text' : type} className="field">
      <Label className="field-label">
        {label}
        {rest.isRequired && <span aria-hidden="true" className="field-required"> *</span>}
      </Label>
      {password ? (
        <span className="field-affix">
          <Input className="field-input" placeholder={placeholder} />
          <Button className="field-reveal" aria-label={shown ? 'Hide password' : 'Show password'} aria-pressed={shown} onPress={() => setShown((s) => !s)} excludeFromTabOrder={false}>
            {shown ? <EyeOff aria-hidden="true" size="1rem" /> : <Eye aria-hidden="true" size="1rem" />}
          </Button>
        </span>
      ) : <Input className="field-input" placeholder={placeholder} />}
      {description && <Text slot="description" className="field-description">{description}</Text>}
      <FieldError className="field-error">{errorMessage}</FieldError>
    </AriaTextField>
  );
}
