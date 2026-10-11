import { X } from 'lucide-react';
import { Dialog, Heading, Modal, ModalOverlay } from 'react-aria-components';

/** A modal form dialog (title, body, footer actions). Focus is trapped and restored by React Aria. */
export function FormDialog({ isOpen, onOpenChange, title, description, children, footer, width = 520 }: {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  children: React.ReactNode;
  footer: React.ReactNode;
  width?: number;
}) {
  return (
    <ModalOverlay isOpen={isOpen} onOpenChange={onOpenChange} isDismissable className="modal-overlay">
      <Modal className="sheet" style={{ width: `min(${width}px, calc(100vw - 32px))` }}>
        <Dialog className="sheet-dialog">
          {({ close }) => (
            <>
              <div className="sheet-head">
                <div>
                  <Heading slot="title" className="sheet-title">{title}</Heading>
                  {description && <p className="sheet-desc">{description}</p>}
                </div>
                <button type="button" className="icon-btn" onClick={close} aria-label="Close"><X aria-hidden="true" size="1rem" /></button>
              </div>
              <div className="sheet-body">{children}</div>
              <div className="sheet-foot">{footer}</div>
            </>
          )}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
