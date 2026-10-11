import { Placeholder } from '@tiptap/extension-placeholder';
import { TableKit } from '@tiptap/extension-table';
import { EditorContent, useEditor, useEditorState } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import { Bold, Heading2, Heading3, ImagePlus, Italic, Link2, List, ListOrdered, MessageSquareQuote, Pilcrow, Table2 } from 'lucide-react';
import { useState } from 'react';
import { type Block, fromEditor, toEditor } from '../lib/blocks';
import { ProcImage } from './DocImageNode';
import { ImagePicker } from './ImagePicker';
import './doc-editor.css';

/**
 * Block editor for SOPs and policies (BPMS-042, ADR-0013; TipTap core, MIT). Edits the portable block
 * model: headings, paragraphs, bullet and numbered lists, callouts, tables, images, bold, italic and links.
 * The toolbar is a single tab stop (arrow keys move between buttons); every button has a name,
 * a pressed state and a keyboard shortcut shown in its tooltip.
 */
export function DocEditor({ initial, onChange, label = 'Content', versionId }: { initial: Block[]; onChange: (blocks: Block[]) => void; label?: string; /** Enables images (attachments of this draft). */ versionId?: string }) {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [2, 3, 4] },
        code: false,
        codeBlock: false,
        strike: false,
        underline: false,
        horizontalRule: false,
        link: { openOnClick: false, autolink: true, protocols: ['mailto'], HTMLAttributes: { rel: 'noopener noreferrer', target: null } },
      }),
      TableKit.configure({ table: { resizable: false } }),
      ProcImage,
      Placeholder.configure({ placeholder: 'Write the purpose, scope and steps. Type # for a heading, - for a list.' }),
    ],
    content: toEditor(initial),
    editorProps: { attributes: { 'aria-label': label, 'aria-multiline': 'true', role: 'textbox', class: 'doc-editor-surface doc-body' } },
    onUpdate: ({ editor: e }) => onChange(fromEditor(e.getJSON())),
    immediatelyRender: true,
  });
  const s = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      bold: e?.isActive('bold') ?? false,
      italic: e?.isActive('italic') ?? false,
      link: e?.isActive('link') ?? false,
      h2: e?.isActive('heading', { level: 2 }) ?? false,
      h3: e?.isActive('heading', { level: 3 }) ?? false,
      paragraph: e?.isActive('paragraph') ?? false,
      bullet: e?.isActive('bulletList') ?? false,
      ordered: e?.isActive('orderedList') ?? false,
      callout: e?.isActive('blockquote') ?? false,
      table: e?.isActive('table') ?? false,
    }),
  });
  const [linking, setLinking] = useState(false);
  const [picking, setPicking] = useState(false);
  const [href, setHref] = useState('');
  if (!editor || !s) return null;
  const c = () => editor.chain().focus();

  const buttons: { name: string; key: string; icon: typeof Bold; on: boolean; run: () => void }[] = [
    { name: 'Text', key: '⌥⌘0', icon: Pilcrow, on: s.paragraph && !s.bullet && !s.ordered && !s.callout, run: () => c().setParagraph().run() },
    { name: 'Heading', key: '⌥⌘2', icon: Heading2, on: s.h2, run: () => c().toggleHeading({ level: 2 }).run() },
    { name: 'Subheading', key: '⌥⌘3', icon: Heading3, on: s.h3, run: () => c().toggleHeading({ level: 3 }).run() },
    { name: 'Bold', key: '⌘B', icon: Bold, on: s.bold, run: () => c().toggleBold().run() },
    { name: 'Italic', key: '⌘I', icon: Italic, on: s.italic, run: () => c().toggleItalic().run() },
    { name: 'Link', key: '⌘K', icon: Link2, on: s.link, run: () => { setHref(String(editor.getAttributes('link').href ?? '')); setLinking(true); } },
    { name: 'Bulleted list', key: '⇧⌘8', icon: List, on: s.bullet, run: () => c().toggleBulletList().run() },
    { name: 'Numbered steps', key: '⇧⌘7', icon: ListOrdered, on: s.ordered, run: () => c().toggleOrderedList().run() },
    { name: 'Callout', key: '⇧⌘B', icon: MessageSquareQuote, on: s.callout, run: () => c().toggleBlockquote().run() },
    { name: 'Table', key: '', icon: Table2, on: s.table, run: () => c().insertTable({ rows: 3, cols: 2, withHeaderRow: true }).run() },
    ...(versionId ? [{ name: 'Image', key: '', icon: ImagePlus, on: false, run: () => setPicking(true) }] : []),
  ];

  const applyLink = () => {
    const h = href.trim();
    if (!h) c().extendMarkRange('link').unsetLink().run();
    else if (/^(https?:|mailto:)/i.test(h) || h.startsWith('/')) c().extendMarkRange('link').setLink({ href: h }).run();
    setLinking(false);
  };

  return (
    <div className="doc-editor">
      <div className="doc-toolbar" role="toolbar" aria-label="Formatting" onKeyDown={(e) => {
        if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
        const all = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('button')];
        const i = all.indexOf(document.activeElement as HTMLButtonElement);
        all[(i + (e.key === 'ArrowRight' ? 1 : all.length - 1)) % all.length]?.focus();
        e.preventDefault();
      }}>
        {buttons.map((b, i) => (
          <button key={b.name} type="button" className="doc-tool" aria-pressed={b.on} aria-label={b.name} title={b.key ? `${b.name} (${b.key})` : b.name} tabIndex={i === 0 ? 0 : -1} onMouseDown={(e) => e.preventDefault()} onClick={b.run}>
            <b.icon aria-hidden="true" size="1rem" />
          </button>
        ))}
        {s.table && (
          <span className="doc-tool-group" aria-label="Table">
            <button type="button" className="doc-tool-text" tabIndex={-1} onClick={() => c().addRowAfter().run()}>Add row</button>
            <button type="button" className="doc-tool-text" tabIndex={-1} onClick={() => c().addColumnAfter().run()}>Add column</button>
            <button type="button" className="doc-tool-text" tabIndex={-1} onClick={() => c().deleteRow().run()}>Delete row</button>
            <button type="button" className="doc-tool-text" tabIndex={-1} onClick={() => c().deleteTable().run()}>Delete table</button>
          </span>
        )}
      </div>
      {linking && (
        <form className="doc-link-bar" onSubmit={(e) => { e.preventDefault(); applyLink(); }}>
          <label htmlFor="doc-link-href" className="visually-hidden">Link address</label>
          <input id="doc-link-href" className="field-input" value={href} onChange={(e) => setHref(e.target.value)} placeholder="https://… or /library/…" autoFocus onKeyDown={(e) => e.key === 'Escape' && setLinking(false)} />
          <button type="submit" className="btn btn-primary btn-md">Apply</button>
          <button type="button" className="btn btn-secondary btn-md" onClick={() => setLinking(false)}>Cancel</button>
        </form>
      )}
      <EditorContent editor={editor} />
      {versionId && <ImagePicker versionId={versionId} isOpen={picking} onClose={() => setPicking(false)} onInsert={(img) => c().insertContent({ type: 'procImage', attrs: img }).run()} />}
    </div>
  );
}
