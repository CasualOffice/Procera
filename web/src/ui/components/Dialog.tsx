import { Dialog as AriaDialog, DialogTrigger, Heading, Modal, ModalOverlay } from 'react-aria-components';
import { TriangleAlert } from 'lucide-react';
import { Button } from './Button';
import './components.css';

/**
 * Confirm dialog. For destructive actions (docs/05 §7.1.1 decision 2): explicit verb on the confirm
 * button, consequence spelled out, Cancel focused first, Esc closes, focus is trapped and restored.
 */
export function ConfirmDialog({
  trigger, title, children, confirmLabel, destructive = false, onConfirm,
}: {
  trigger: React.ReactElement; title: string; children: React.ReactNode; confirmLabel: string; destructive?: boolean; onConfirm: () => void;
}) {
  return (
    <DialogTrigger>
      {trigger}
      <ModalOverlay className="modal-overlay" isDismissable>
        <Modal className="modal">
          <AriaDialog className="dialog" role={destructive ? 'alertdialog' : 'dialog'}>
            {({ close }) => (
              <>
                <Heading slot="title" className="dialog-title">
                  {destructive && <TriangleAlert aria-hidden="true" size="1.25rem" className="dialog-icon" />}
                  {title}
                </Heading>
                <div className="dialog-body">{children}</div>
                <div className="dialog-actions">
                  <Button variant="secondary" autoFocus onPress={close}>Cancel</Button>
                  <Button variant={destructive ? 'danger' : 'primary'} onPress={() => { onConfirm(); close(); }}>{confirmLabel}</Button>
                </div>
              </>
            )}
          </AriaDialog>
        </Modal>
      </ModalOverlay>
    </DialogTrigger>
  );
}
