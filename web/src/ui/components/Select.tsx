import { Button, Label, ListBox, ListBoxItem, Popover, Select as AriaSelect, SelectValue, Text, type SelectProps as AriaSelectProps } from 'react-aria-components';
import { ChevronDown } from 'lucide-react';
import './components.css';

export interface Option { id: string; label: string }

export interface SelectProps extends Omit<AriaSelectProps<Option>, 'children'> {
  label: string;
  description?: string;
  options: Option[];
}

/** Single select (React Aria): keyboard type-ahead, arrow navigation, screen-reader announced. */
export function Select({ label, description, options, ...rest }: SelectProps) {
  return (
    <AriaSelect {...rest} className="field select">
      <Label className="field-label">{label}</Label>
      <Button className="field-input select-trigger">
        <SelectValue />
        <ChevronDown aria-hidden="true" size="1.125rem" />
      </Button>
      {description && <Text slot="description" className="field-description">{description}</Text>}
      <Popover className="popover">
        <ListBox className="listbox" items={options}>
          {(o) => <ListBoxItem id={o.id} className="listbox-item">{o.label}</ListBoxItem>}
        </ListBox>
      </Popover>
    </AriaSelect>
  );
}
