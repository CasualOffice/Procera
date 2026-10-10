import { FieldError, Input, Label, Text, TextField as AriaTextField, type TextFieldProps as AriaTextFieldProps } from 'react-aria-components';
import './components.css';

export interface TextFieldProps extends AriaTextFieldProps {
  label: string;
  description?: string;
  errorMessage?: string;
}

/** Labelled text input with description and error wired through ARIA (never placeholder-as-label). */
export function TextField({ label, description, errorMessage, ...rest }: TextFieldProps) {
  return (
    <AriaTextField {...rest} className="field">
      <Label className="field-label">
        {label}
        {rest.isRequired && <span aria-hidden="true" className="field-required"> *</span>}
      </Label>
      <Input className="field-input" />
      {description && <Text slot="description" className="field-description">{description}</Text>}
      <FieldError className="field-error">{errorMessage}</FieldError>
    </AriaTextField>
  );
}
