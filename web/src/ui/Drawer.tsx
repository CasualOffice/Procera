import { X } from 'lucide-react';
import { Dialog, Heading, Modal, ModalOverlay } from 'react-aria-components';

/**
 * A side panel over the list (docs/05 §3.14 "detail drawer"): the list stays in view, Esc or the
 * close button returns to it, focus is trapped and restored by React Aria. Full screen on phones.
 */
export function Drawer({ title, onClose, children, footer }: { title: string; onClose: () => void; children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <ModalOverlay isOpen onOpenChange={(o) => !o && onClose()} isDismissable className="drawer-overlay">
      <Modal className="drawer">
        <Dialog className="drawer-dialog">
          <div className="drawer-head">
            <Heading slot="title" className="drawer-title">{title}</Heading>
            <button type="button" className="icon-btn" onClick={onClose} aria-label="Close"><X aria-hidden="true" size="1rem" /></button>
          </div>
          <div className="drawer-body">{children}</div>
          {footer && <div className="drawer-foot">{footer}</div>}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
