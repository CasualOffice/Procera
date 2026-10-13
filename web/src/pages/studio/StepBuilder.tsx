import { ArrowDown, ArrowUp, CircleAlert, CornerDownRight, Ellipsis, Filter, Flag, Plus, Send, Trash2, UserRound } from 'lucide-react';
import { Button as AriaButton, Menu, MenuItem, MenuTrigger, Popover, Separator } from 'react-aria-components';
import type { Problem } from './api';
import { type Model, type Step, type StepKind, STEP_KINDS, deadlineText, describeCondition, routes, whoText } from './model';
import { EndIcon, KIND_LOOK } from './parts';

type Action = { type: 'add'; after: number; kind: StepKind } | { type: 'move'; id: string; by: -1 | 1 } | { type: 'remove'; id: string };

/**
 * The centre column (docs/05 §3.5): the steps in order as a vertical flow, from "Request submitted"
 * to "Approved", each with who does it, its deadline and where each outcome goes. Every change has a
 * keyboard path: the step menu ("Add … after", "Move up/down", "Delete") and Alt+↑/↓ on a step.
 */
export function StepBuilder({ model, selected, onSelect, onAction, problems, readOnly, roleName }: {
  model: Model; selected: string | null; onSelect: (id: string) => void; onAction: (a: Action) => void;
  problems: Map<string, Problem[]>; readOnly: boolean; roleName: (key: string) => string | undefined;
}) {
  const anyApproval = model.steps.some((s) => s.kind === 'approval');
  return (
    <div className="studio-flow-wrap">
      <ol className="studio-flow" aria-label="Steps">
        <li className="flow-cap flow-start">
          <span className="flow-dot" aria-hidden="true"><Send size="0.8125rem" /></span>
          <div>
            <p className="flow-cap-title">Request submitted</p>
            <p className="flow-cap-sub">Someone fills in the {model.request.name.trim() || 'request'} form{model.fields.length ? ` (${model.fields.length} ${model.fields.length === 1 ? 'question' : 'questions'})` : ''}.</p>
          </div>
          {!readOnly && <AddStepMenu label="Add a first step" compact onAdd={(kind) => onAction({ type: 'add', after: -1, kind })} />}
        </li>
        {model.steps.map((s, i) => (
          <StepCard key={s.uid} model={model} step={s} index={i} selected={selected === s.id} onSelect={onSelect} onAction={onAction}
            problems={problems.get(s.id) ?? []} readOnly={readOnly} roleName={roleName} />
        ))}
        {!model.steps.length && (
          <li className="flow-empty">
            <p>No steps yet, so a request would be approved as soon as it’s submitted.</p>
            <p className="muted">Add an approval to have someone decide it.</p>
          </li>
        )}
        {!readOnly && (
          <li className="flow-add">
            <AddStepMenu label="Add step" onAdd={(kind) => onAction({ type: 'add', after: model.steps.length - 1, kind })} />
          </li>
        )}
        <li className="flow-cap flow-end">
          <span className="flow-dot flow-dot-end" aria-hidden="true"><EndIcon size="0.875rem" /></span>
          <div>
            <p className="flow-cap-title">Approved</p>
            <p className="flow-cap-sub">{anyApproval ? 'The request is done. A rejection at any approval ends it as Rejected; sending back returns it to the requester, then to the first step.' : 'The request is done.'}</p>
          </div>
        </li>
      </ol>
    </div>
  );
}

function StepCard({ model, step: s, index, selected, onSelect, onAction, problems, readOnly, roleName }: {
  model: Model; step: Step; index: number; selected: boolean; onSelect: (id: string) => void; onAction: (a: Action) => void;
  problems: Problem[]; readOnly: boolean; roleName: (key: string) => string | undefined;
}) {
  const look = KIND_LOOK[s.kind];
  const last = index === model.steps.length - 1;
  const errors = problems.filter((p) => p.severity === 'error');
  const name = s.name.trim() || 'Untitled step';
  return (
    <li className={`flow-step${selected ? ' is-selected' : ''}${errors.length ? ' has-error' : ''}`} data-kind={s.kind}>
      {s.when && (
        <p className="flow-when"><Filter aria-hidden="true" size="0.75rem" /> Only when {describeCondition(s.when, model.fields)}; otherwise skipped</p>
      )}
      <div className="step-card">
        <div className="step-card-top">
          <span className={`step-kind tag tag-${look.color}`}><look.Icon aria-hidden="true" size="0.75rem" />{look.label}</span>
          <span className="step-index" aria-hidden="true">{index + 1}</span>
        </div>
        <h3 className="step-title">
          <button type="button" id={`step-${s.id}`} className="step-select" aria-pressed={selected} aria-describedby={`step-${s.id}-sum`}
            onClick={() => onSelect(s.id)}
            onKeyDown={(e) => {
              if (readOnly || !e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return;
              e.preventDefault();
              onAction({ type: 'move', id: s.id, by: e.key === 'ArrowUp' ? -1 : 1 });
            }}>
            {name}
          </button>
        </h3>
        <div id={`step-${s.id}-sum`} className="step-sum">
          <p className="step-who"><UserRound aria-hidden="true" size="0.8125rem" />{s.who.length ? s.who.map((w) => whoText(w, roleName)).join(', or ') : 'Nobody chosen yet'}</p>
          <p className="step-meta"><Flag aria-hidden="true" size="0.8125rem" />{deadlineText(s)}</p>
          <ul className="step-routes" aria-label={`Outcomes of ${name}`}>
            {routes(model, s).map((r) => (
              <li key={r.outcome}><span className="step-route-label">{r.label}</span><CornerDownRight aria-hidden="true" size="0.75rem" /><span>{r.to}</span></li>
            ))}
          </ul>
        </div>
        {errors.map((p, i) => (
          <p key={i} className="step-problem"><CircleAlert aria-hidden="true" size="0.875rem" /><span><span className="visually-hidden">Problem: </span>{p.message}</span></p>
        ))}
        {!readOnly && (
          <MenuTrigger>
            <AriaButton className="icon-btn step-menu-btn" aria-label={`Actions for ${name}`}><Ellipsis aria-hidden="true" size="1rem" /></AriaButton>
            <Popover className="menu-popover" placement="bottom end">
              <Menu className="menu" aria-label={`Actions for ${name}`} onAction={(k) => {
                const key = String(k);
                if (key.startsWith('add-')) onAction({ type: 'add', after: index, kind: key.slice(4) as StepKind });
                else if (key === 'up') onAction({ type: 'move', id: s.id, by: -1 });
                else if (key === 'down') onAction({ type: 'move', id: s.id, by: 1 });
                else if (key === 'delete') onAction({ type: 'remove', id: s.id });
              }}>
                {STEP_KINDS.map((k) => (
                  <MenuItem key={k.id} id={`add-${k.id}`} className="menu-item" textValue={`Add ${k.label.toLowerCase()} after`}><Plus aria-hidden="true" size="0.875rem" /> Add {k.label.toLowerCase()} after</MenuItem>
                ))}
                <Separator className="menu-separator" />
                <MenuItem id="up" className="menu-item" isDisabled={index === 0} textValue="Move up"><ArrowUp aria-hidden="true" size="0.875rem" /> Move up<kbd className="menu-item-meta">Alt ↑</kbd></MenuItem>
                <MenuItem id="down" className="menu-item" isDisabled={last} textValue="Move down"><ArrowDown aria-hidden="true" size="0.875rem" /> Move down<kbd className="menu-item-meta">Alt ↓</kbd></MenuItem>
                <Separator className="menu-separator" />
                <MenuItem id="delete" className="menu-item menu-item-danger" textValue="Delete step"><Trash2 aria-hidden="true" size="0.875rem" /> Delete step</MenuItem>
              </Menu>
            </Popover>
          </MenuTrigger>
        )}
      </div>
    </li>
  );
}

function AddStepMenu({ label, onAdd, compact = false }: { label: string; onAdd: (kind: StepKind) => void; compact?: boolean }) {
  return (
    <MenuTrigger>
      <AriaButton className={compact ? 'flow-add-mini' : 'btn btn-secondary btn-md flow-add-btn'} aria-label={compact ? label : undefined}>
        <Plus aria-hidden="true" size="0.875rem" />{!compact && ` ${label}`}
      </AriaButton>
      <Popover className="menu-popover" placement="bottom start">
        <Menu className="menu" aria-label={label} onAction={(k) => onAdd(String(k) as StepKind)}>
          {STEP_KINDS.map((k) => {
            const Icon = KIND_LOOK[k.id].Icon;
            return (
              <MenuItem key={k.id} id={k.id} className="menu-item studio-kind-item" textValue={k.label}>
                <Icon aria-hidden="true" size="0.875rem" />
                <span className="studio-kind-text"><span>{k.label}</span><span className="studio-kind-hint">{k.hint}</span></span>
              </MenuItem>
            );
          })}
        </Menu>
      </Popover>
    </MenuTrigger>
  );
}

export type StepAction = Action;
