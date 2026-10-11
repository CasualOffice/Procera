import { Node, mergeAttributes } from '@tiptap/core';
import { NodeViewWrapper, ReactNodeViewRenderer, type ReactNodeViewProps } from '@tiptap/react';
import { DocImage } from './DocImage';

function ImageView({ node, selected }: ReactNodeViewProps) {
  const { fileId, alt, caption } = node.attrs as { fileId: string; alt: string; caption: string };
  return (
    <NodeViewWrapper className="doc-figure-node" data-selected={selected ? '' : undefined} contentEditable={false}>
      <DocImage fileId={fileId} alt={alt} caption={caption || undefined} />
    </NodeViewWrapper>
  );
}

/** The editor's image block (an atom): file id, alt text and caption, as in the block model. */
export const ProcImage = Node.create({
  name: 'procImage',
  group: 'block',
  atom: true,
  draggable: true,
  selectable: true,
  addAttributes() {
    return { fileId: { default: '' }, alt: { default: '' }, caption: { default: '' } };
  },
  parseHTML() {
    return [{ tag: 'figure[data-file-id]', getAttrs: (el) => ({ fileId: (el as HTMLElement).dataset.fileId ?? '', alt: (el as HTMLElement).dataset.alt ?? '' }) }];
  },
  renderHTML({ HTMLAttributes }) {
    return ['figure', mergeAttributes({ 'data-file-id': HTMLAttributes.fileId, 'data-alt': HTMLAttributes.alt })];
  },
  addNodeView() {
    return ReactNodeViewRenderer(ImageView);
  },
});
