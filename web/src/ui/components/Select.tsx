import { Button, FieldError, Label, ListBox, ListBoxItem, Popover, Select as AriaSelect, SelectValue, Text, type SelectProps as AriaSelectProps } from 'react-aria-components';
import { ChevronDown } from 'lucide-react';
import './components.css';

export interface Option { id: string; label: string }

export interface SelectProps extends Omit<AriaSelectProps<Option>, 'children'> {
  label: string;
  description?: string;
  options: Option[];
  errorMessage?: string;
}

/** Single select (React Aria): keyboard type-ahead, arrow navigation, screen-reader announced. */
export function Select({ label, description, options, errorMessage, ...rest }: SelectProps) {
  return (
    <AriaSelect {...rest} className="field select">
      <Label className="field-label">{label}{rest.isRequired && <span aria-hidden="true" className="field-required"> *</span>}</Label>
      <Button className="field-input select-trigger">
        <SelectValue />
        <ChevronDown aria-hidden="true" size="1.125rem" />
      </Button>
      {description && <Text slot="description" className="field-description">{description}</Text>}
      <FieldError className="field-error">{errorMessage}</FieldError>
      <Popover className="popover">
        <ListBox className="listbox" items={options}>
          {(o) => <ListBoxItem id={o.id} className="listbox-item">{o.label}</ListBoxItem>}
        </ListBox>
      </Popover>
    </AriaSelect>
  );
}
