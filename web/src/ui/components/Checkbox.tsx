import { Checkbox as AriaCheckbox, type CheckboxProps as AriaCheckboxProps } from 'react-aria-components';
import { Check } from 'lucide-react';
import './components.css';

export function Checkbox({ children, ...rest }: AriaCheckboxProps & { children: React.ReactNode }) {
  return (
    <AriaCheckbox {...rest} className="checkbox">
      {({ isSelected }) => (
        <>
          <span className="box" aria-hidden="true">{isSelected && <Check size="0.75rem" strokeWidth={3.5} />}</span>
          <span>{children}</span>
        </>
      )}
    </AriaCheckbox>
  );
}
