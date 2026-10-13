import {
  Background, BackgroundVariant, type Edge, EdgeLabelRenderer, type EdgeProps, Handle, MiniMap, type Node, type NodeProps, Panel, Position, ReactFlow,
  ReactFlowProvider, useReactFlow, useStoreApi,
} from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import { Bell, CircleAlert, CircleX, CornerUpLeft, Filter, Flag, GitFork, GitMerge, Map as MapIcon, Scan, Send, UserRound, Workflow, ZoomIn, ZoomOut } from 'lucide-react';
import { type KeyboardEvent, createContext, useContext, useEffect, useMemo, useRef, useState } from 'react';
import type { Problem } from './api';
import { type CvEdge, type CvGraph, type CvNode, type Direction, type Pt, irGraph, modelGraph, neighbour } from './canvasLayout';
import type { Model } from './model';
import { EndIcon, KIND_LOOK } from './parts';
import { AddStepMenu, type StepAction, StepMenu } from './StepBuilder';
import './canvas.css';

type CvFlowNode = Node<{ node: CvNode }, 'cv'>;
type CvFlowEdge = Edge<{ e: CvEdge; px: number }, 'route'>;

interface Ctx {
  model: Model; graph: CvGraph; px: number; selected: string | null; editable: boolean; motion: number;
  onSelect: (id: string) => void; onAction: (a: StepAction) => void; problems: Map<string, Problem[]>;
}
const CanvasCtx = createContext<Ctx | null>(null);

/** The element a node is focused by: a step keeps the id the step list uses, so focus code works in both views. */
const domId = (n: CvNode) => (n.kind === 'step' && n.stepId ? `step-${n.stepId}` : `cv-${n.id}`);
/** The background dots' radius, in canvas units (it scales with the zoom, unlike CSS lengths). */
const DOT = 1;
const ARROWS: Record<string, Direction> = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };

function rootPx(): number {
  try { return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16; } catch { return 16; }
}
function reducedMotion(): boolean {
  try { return window.matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return false; }
}

/**
 * The designer's flow canvas (BPMS-100, docs/05 §3.5; React Flow per ADR-0018): the same model as
 * the step list, drawn as a graph. The structure is read-only here (adding and moving stay in the
 * step menus, which every step node has too); a step node selects the step for the settings panel.
 * Keyboard: Tab walks the nodes in flow order, arrow keys follow the connections, Enter or Space
 * selects, Alt+↑/↓ moves a step. Nothing needs dragging; the toolbar zooms and fits.
 */
export function StudioCanvas(props: {
  model: Model; selected: string | null; onSelect: (id: string) => void; onAction: (a: StepAction) => void;
  problems: Map<string, Problem[]>; readOnly: boolean; roleName: (key: string) => string | undefined; narrow?: boolean;
}) {
  return <ReactFlowProvider><Canvas {...props} /></ReactFlowProvider>;
}

function Canvas({ model, selected, onSelect, onAction, problems, readOnly, roleName, narrow = false }: Parameters<typeof StudioCanvas>[0]) {
  const [px, setPx] = useState(rootPx);
  const [mini, setMini] = useState(false);
  const flow = useReactFlow();
  const motion = reducedMotion() ? 0 : 200;
  useEffect(() => {
    const on = () => setPx(rootPx());
    window.addEventListener('resize', on);
    return () => window.removeEventListener('resize', on);
  }, []);

  const graph = useMemo(() => (model.readOnly ? irGraph(model.base, roleName) : modelGraph(model, roleName)), [model, roleName]);
  const nodes = useMemo<CvFlowNode[]>(() => graph.nodes.map((n) => ({
    id: n.id, type: 'cv', position: { x: n.x * px, y: n.y * px }, width: n.w * px, height: n.h * px, data: { node: n },
    draggable: false, selectable: false, connectable: false, focusable: false, domAttributes: { 'aria-roledescription': undefined },
  })), [graph, px]);
  const edges = useMemo<CvFlowEdge[]>(() => {
    const name = (id: string) => graph.nodes.find((n) => n.id === id)?.label ?? id;
    return graph.edges.map((e) => ({
      id: e.id, source: e.from, target: e.to, type: 'route', data: { e, px }, selectable: false, focusable: false,
      ariaLabel: `${name(e.from)}${e.label ? `, ${e.label},` : ''} to ${name(e.to)}`,
    }));
  }, [graph, px]);

  // A step added or removed changes the drawing's size: fit it again.
  const count = nodes.length;
  const first = useRef(true);
  useEffect(() => {
    if (first.current) { first.current = false; return; }
    const t = requestAnimationFrame(() => void flow.fitView({ padding: 0.12, maxZoom: 1, duration: motion }));
    return () => cancelAnimationFrame(t);
  }, [count, flow, motion]);

  const ctx: Ctx = { model, graph, px, selected, editable: !readOnly && !model.readOnly, motion, onSelect, onAction, problems };
  return (
    <section className={`cv${narrow ? ' cv-narrow' : ''}`} aria-label="Process flow" aria-describedby="cv-hint">
      <CanvasCtx.Provider value={ctx}>
        <ReactFlow<CvFlowNode, CvFlowEdge>
          nodes={nodes} edges={edges} nodeTypes={NODE_TYPES} edgeTypes={EDGE_TYPES} aria-label="Process flow diagram"
          nodesDraggable={false} nodesConnectable={false} nodesFocusable={false} edgesFocusable={false} elementsSelectable={false} disableKeyboardA11y
          deleteKeyCode={null} selectionKeyCode={null} multiSelectionKeyCode={null} panActivationKeyCode={null}
          zoomOnScroll={false} preventScrolling={false} zoomOnDoubleClick={false} fitView fitViewOptions={{ padding: 0.12, maxZoom: 1 }}
          minZoom={0.2} maxZoom={1.75} attributionPosition="bottom-right"
          // A click anywhere on a step (or its condition) selects it; this also keeps nodes clickable (no drag, no RF selection).
          onNodeClick={(_, node) => { const id = node.data.node.stepId; if (id) onSelect(id); }}>
          <Background variant={BackgroundVariant.Dots} gap={18} size={DOT} />
          <Panel position="top-right">
            <div className="cv-controls" role="group" aria-label="Zoom">
              <button type="button" className="icon-btn" aria-label="Zoom in" title="Zoom in" onClick={() => void flow.zoomIn({ duration: motion })}><ZoomIn aria-hidden="true" size="1rem" /></button>
              <button type="button" className="icon-btn" aria-label="Zoom out" title="Zoom out" onClick={() => void flow.zoomOut({ duration: motion })}><ZoomOut aria-hidden="true" size="1rem" /></button>
              <button type="button" className="icon-btn" aria-label="Fit to view" title="Fit to view" onClick={() => void flow.fitView({ padding: 0.12, maxZoom: 1, duration: motion })}><Scan aria-hidden="true" size="1rem" /></button>
              <span className="cv-controls-sep" aria-hidden="true" />
              <button type="button" className="icon-btn" aria-label="Minimap" title="Minimap" aria-pressed={mini} onClick={() => setMini((v) => !v)}><MapIcon aria-hidden="true" size="1rem" /></button>
            </div>
          </Panel>
          {mini && <MiniMap<CvFlowNode> position="bottom-left" pannable zoomable ariaLabel="Minimap of the process flow" nodeClassName={(n) => `cv-mini cv-mini-${n.data.node.kind}`} />}
        </ReactFlow>
      </CanvasCtx.Provider>
      <p id="cv-hint" className="cv-hint">
        {narrow ? 'Drag to move around, pinch to zoom.' : `Tab moves through the flow, arrow keys follow the arrows${ctx.editable ? ', Enter opens a step’s settings and Alt+↑/↓ moves it' : ''}. Drag or use the buttons to move around.`}
      </p>
    </section>
  );
}

/* ---------------------------------------------------------------- nodes */

function useNodeKeys(n: CvNode) {
  const ctx = useContext(CanvasCtx)!;
  const flow = useReactFlow();
  const store = useStoreApi();
  const onFocus = () => {
    // Keep the focused node in view (the canvas never scrolls; it pans).
    const { transform: [tx, ty, zoom], width, height } = store.getState();
    const left = n.x * ctx.px * zoom + tx;
    const top = n.y * ctx.px * zoom + ty;
    if (left < 0 || top < 0 || left + n.w * ctx.px * zoom > width || top + n.h * ctx.px * zoom > height) {
      void flow.setCenter((n.x + n.w / 2) * ctx.px, (n.y + n.h / 2) * ctx.px, { zoom, duration: ctx.motion });
    }
  };
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.target !== e.currentTarget) return;
    const dir = ARROWS[e.key];
    if (!dir || e.metaKey || e.ctrlKey || e.shiftKey) return;
    if (e.altKey) {
      if (ctx.editable && n.kind === 'step' && n.stepId && (dir === 'up' || dir === 'down')) {
        e.preventDefault();
        ctx.onAction({ type: 'move', id: n.stepId, by: dir === 'up' ? -1 : 1 });
      }
      return;
    }
    e.preventDefault();
    const to = neighbour(ctx.graph, n.id, dir);
    const target = to ? ctx.graph.nodes.find((x) => x.id === to) : undefined;
    if (target) document.getElementById(domId(target))?.focus();
  };
  return { ctx, focus: { id: domId(n), onFocus, onKeyDown } };
}

function Handles() {
  return (
    <>
      <Handle type="target" position={Position.Top} isConnectable={false} className="cv-handle" />
      <Handle type="source" position={Position.Bottom} isConnectable={false} className="cv-handle" />
    </>
  );
}

function CvNodeView({ data }: NodeProps<CvFlowNode>) {
  const n = data.node;
  switch (n.kind) {
    case 'step': return <StepNode n={n} />;
    case 'decision': return <DecisionNode n={n} />;
    case 'start': return <StartNode n={n} />;
    default: return <PlainNode n={n} />;
  }
}

function StepNode({ n }: { n: CvNode }) {
  const { ctx, focus } = useNodeKeys(n);
  const look = KIND_LOOK[n.stepKind ?? 'task'];
  const errors = (n.stepId ? ctx.problems.get(n.stepId) ?? [] : []).filter((p) => p.severity === 'error');
  const index = n.stepId ? ctx.model.steps.findIndex((s) => s.id === n.stepId) : -1;
  const selected = !!n.stepId && ctx.selected === n.stepId;
  const sum = `${focus.id}-sum`;
  const summary = (
    <div id={sum} className="cv-sum">
      <p><UserRound aria-hidden="true" size="0.8125rem" /><span>{n.who}</span></p>
      <p><Flag aria-hidden="true" size="0.8125rem" /><span>{n.deadline}</span></p>
      {errors.map((p, i) => <span key={i} className="visually-hidden">Problem: {p.message}</span>)}
    </div>
  );
  return (
    <div className={`cv-card cv-step${selected ? ' is-selected' : ''}${errors.length ? ' has-error' : ''}`} data-kind={n.stepKind}>
      <Handles />
      <div className="cv-card-top">
        <span className={`step-kind tag tag-${look.color}`}><look.Icon aria-hidden="true" size="0.75rem" />{look.label}</span>
        {errors.length > 0 && <CircleAlert className="cv-problem" aria-hidden="true" size="0.9375rem" />}
      </div>
      {n.stepId ? (
        <p className="cv-title">
          <button type="button" className="cv-select" {...focus} aria-pressed={selected} aria-describedby={sum} onClick={() => ctx.onSelect(n.stepId!)}>{n.label}</button>
        </p>
      ) : (
        <p className="cv-title"><span className="cv-select cv-focusable" tabIndex={0} role="group" aria-label={`${n.label} (${look.label.toLowerCase()})`} aria-describedby={sum} {...focus}>{n.label}</span></p>
      )}
      {summary}
      {ctx.editable && n.stepId && index >= 0 && <StepMenu name={n.label} index={index} count={ctx.model.steps.length} stepId={n.stepId} onAction={ctx.onAction} />}
    </div>
  );
}

function DecisionNode({ n }: { n: CvNode }) {
  const { ctx, focus } = useNodeKeys(n);
  const body = (
    <>
      <span className="cv-when-text">{n.label}</span>
      <span className="cv-diamond" aria-hidden="true"><Filter size="0.75rem" /></span>
      <span />
    </>
  );
  const step = n.stepId ? ctx.model.steps.find((s) => s.id === n.stepId) : undefined;
  return (
    <div className="cv-decision">
      <Handles />
      {step ? (
        <button type="button" className="cv-decision-body" {...focus} aria-label={`${n.label}: condition of ${step.name.trim() || 'Untitled step'}`} onClick={() => ctx.onSelect(step.id)}>{body}</button>
      ) : (
        <div className="cv-decision-body cv-focusable" tabIndex={0} role="group" aria-label={`Decision: ${n.label}`} {...focus}>{body}</div>
      )}
    </div>
  );
}

function StartNode({ n }: { n: CvNode }) {
  const { ctx, focus } = useNodeKeys(n);
  return (
    <div className="cv-card cv-cap">
      <Handles />
      <div className="cv-cap-main cv-focusable" tabIndex={0} role="group" aria-label={n.label} {...focus}>
        <span className="cv-dot" aria-hidden="true"><Send size="0.8125rem" /></span>
        <span className="cv-cap-text"><span className="cv-cap-title">{n.label}</span>{n.sub && <span className="cv-cap-sub">{n.sub}</span>}</span>
      </div>
      {ctx.editable && !ctx.model.steps.length && <AddStepMenu label="Add a first step" compact onAdd={(kind) => ctx.onAction({ type: 'add', after: -1, kind })} />}
    </div>
  );
}

function PlainNode({ n }: { n: CvNode }) {
  const { focus } = useNodeKeys(n);
  const Icon = n.kind === 'end' ? (n.tone === 'stopped' ? CircleX : EndIcon) : n.kind === 'revise' ? CornerUpLeft : n.kind === 'gateway' ? (n.tone === 'join' ? GitMerge : GitFork) : n.kind === 'notify' ? Bell : Workflow;
  const what = n.kind === 'end' ? 'End' : n.kind === 'revise' ? 'Returns to the requester' : n.kind === 'gateway' ? (n.tone === 'join' ? 'Waits for all branches' : 'Runs branches at the same time') : n.kind === 'notify' ? 'Notification' : 'Step';
  return (
    <div className={`cv-plain cv-${n.kind}${n.tone ? ` is-${n.tone}` : ''} cv-focusable`} tabIndex={0} role="group" aria-label={`${n.label} (${what.toLowerCase()})`} {...focus}>
      <Handles />
      <span className="cv-plain-icon" aria-hidden="true"><Icon size="0.875rem" /></span>
      <span className="cv-plain-text">
        <span className="cv-plain-title">{n.label}</span>
        {n.sub && <span className="cv-plain-sub">{n.sub}</span>}
      </span>
    </div>
  );
}

/* ---------------------------------------------------------------- edges */

const toward = (a: Pt, b: Pt, d: number): Pt => {
  const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
  return [a[0] + ((b[0] - a[0]) / len) * d, a[1] + ((b[1] - a[1]) / len) * d];
};

/** An orthogonal polyline with rounded corners. */
function rounded(p: Pt[], r: number): string {
  let d = `M${p[0]![0]},${p[0]![1]}`;
  for (let i = 1; i < p.length - 1; i++) {
    const [prev, cur, next] = [p[i - 1]!, p[i]!, p[i + 1]!];
    const k = Math.min(r, Math.hypot(cur[0] - prev[0], cur[1] - prev[1]) / 2, Math.hypot(next[0] - cur[0], next[1] - cur[1]) / 2);
    const a = toward(cur, prev, k);
    const b = toward(cur, next, k);
    d += ` L${a[0]},${a[1]} Q${cur[0]},${cur[1]} ${b[0]},${b[1]}`;
  }
  const last = p[p.length - 1]!;
  return `${d} L${last[0]},${last[1]}`;
}

function RouteEdge({ data }: EdgeProps<CvFlowEdge>) {
  if (!data || data.e.points.length < 2) return null;
  const { e, px } = data;
  const pts = e.points.map(([x, y]): Pt => [x * px, y * px]);
  const tip = pts[pts.length - 1]!;
  const back = toward(tip, pts[pts.length - 2]!, 0.5 * px);
  const [ux, uy] = [(tip[0] - back[0]) / (0.5 * px), (tip[1] - back[1]) / (0.5 * px)];
  const w = 0.28 * px;
  const arrow = `M${tip[0]},${tip[1]} L${back[0] - uy * w},${back[1] + ux * w} L${back[0] + uy * w},${back[1] - ux * w} Z`;
  return (
    <>
      <path d={rounded(pts, 0.5 * px)} className="cv-edge" />
      <path d={arrow} className="cv-arrow" />
      {e.label && e.labelAt && (
        <EdgeLabelRenderer>
          <div aria-hidden="true" className={`cv-edge-label nodrag nopan${e.labelAlign === 'end' ? ' is-end' : ''}`}
            style={{ transform: `translate(${e.labelAt[0] * px}px, ${e.labelAt[1] * px}px)${e.labelAlign === 'end' ? ' translateX(-100%)' : ''}` }}>
            {e.label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}

const NODE_TYPES = { cv: CvNodeView };
const EDGE_TYPES = { route: RouteEdge };
