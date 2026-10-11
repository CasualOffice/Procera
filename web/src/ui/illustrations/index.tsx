import type { CSSProperties, ReactNode } from 'react';
import './illustrations.css';

/**
 * Empty-state illustrations (docs/05 §3 state kit). One small drawing per kind of empty state, in the
 * marketing site's language: ink outlines, paper cards with a hard offset shadow, the tag pastels,
 * dashed strokes for "not yet". Decorative only: the Empty's text carries the meaning.
 * Drawn on a 128 × 96 grid; strokes are 1.5 units. Motion lives in illustrations.css.
 */
export type ArtKind =
  | 'library' | 'caught-up' | 'announcements' | 'notifications' | 'campaigns' | 'people' | 'departments'
  | 'groups' | 'roles' | 'templates' | 'files' | 'audit' | 'no-match' | 'generic';

type Tone = 0 | 1 | 2 | 3 | 4 | 5 | 6;
/** Entrance delay for an element, read by the motion classes. */
const d = (ms: number) => ({ '--d': `${ms}ms` }) as CSSProperties;

function Card({ x, y, w, h, r = 5, c = 'pp', shadow = true }: { x: number; y: number; w: number; h: number; r?: number; c?: string; shadow?: boolean }) {
  return (
    <>
      {shadow && <rect className="sh" x={x + 3} y={y + 3} width={w} height={h} rx={r} />}
      <rect className={c} x={x} y={y} width={w} height={h} rx={r} />
    </>
  );
}

function Lines({ x, y, ws, gap = 6 }: { x: number; y: number; ws: number[]; gap?: number }) {
  return <>{ws.map((w, i) => <rect key={i} className="ln" x={x} y={y + i * gap} width={w} height={2.5} rx={1.25} />)}</>;
}

/** A pastel check badge that pops in, then draws its tick. */
function Check({ cx, cy, r = 7, tone = 1, at = 0 }: { cx: number; cy: number; r?: number; tone?: Tone; at?: number }) {
  const s = r / 7;
  return (
    <g className="a-pop" style={d(at)}>
      <circle className={`t${tone}`} cx={cx} cy={cy} r={r} />
      <circle className="k" cx={cx} cy={cy} r={r} />
      <path className={`s${tone} a-draw`} style={{ ...d(at + 140), strokeWidth: 2.25 }} pathLength={1}
        d={`M${cx - 3.2 * s} ${cy + 0.2 * s}l${2.2 * s} ${2.2 * s} ${4.4 * s} -${4.6 * s}`} />
    </g>
  );
}

/** A person: pastel disc, ink outline, head-and-shoulders glyph in the pastel's text colour. */
function Avatar({ cx, cy, r, tone }: { cx: number; cy: number; r: number; tone: Tone }) {
  return (
    <>
      <circle className={`t${tone}`} cx={cx} cy={cy} r={r} />
      <circle className={`f${tone}`} cx={cx} cy={cy - r * 0.22} r={r * 0.3} />
      <path className={`f${tone}`} d={`M${cx - r * 0.52} ${cy + r * 0.62}a${r * 0.52} ${r * 0.46} 0 0 1 ${r * 1.04} 0z`} />
      <circle className="k" cx={cx} cy={cy} r={r} />
    </>
  );
}

function Library() {
  return (
    <>
      <g transform="rotate(-8 52 50)"><g className="a-rise"><Card x={32} y={24} w={40} h={52} c="ps" /></g></g>
      <g className="a-rise" style={d(110)}>
        <Card x={48} y={18} w={42} h={56} />
        <rect className="t2" x={54} y={25} width={16} height={5} rx={2.5} />
        <rect className="kx" x={54} y={34} width={24} height={3.5} rx={1.5} />
        <rect className="rd" x={80} y={33} width={1.5} height={5.5} rx={0.75} />
        <Lines x={54} y={44} ws={[30, 24, 28, 16]} />
      </g>
      <g className="a-pop" style={d(460)}>
        <circle className="pp" cx={90} cy={72} r={8} />
        <path className="k" d="M86.5 72h7M90 68.5v7" />
      </g>
    </>
  );
}

function CaughtUp() {
  const rows = [30, 42, 54];
  return (
    <>
      <g className="a-rise"><Card x={34} y={20} w={52} h={56} /></g>
      {rows.map((y, i) => (
        <g key={y} className="a-rise" style={d(90 + i * 70)}>
          <rect className="t1" x={41} y={y - 3.5} width={7} height={7} rx={2} />
          <rect className="k" x={41} y={y - 3.5} width={7} height={7} rx={2} />
          <path className="s1" style={{ strokeWidth: 1.5 }} d={`M42.8 ${y}l1.6 1.6 2.8-3`} />
          <rect className="ln" x={53} y={y - 1.25} width={[24, 18, 21][i]} height={2.5} rx={1.25} />
        </g>
      ))}
      <circle className="k a-burst" style={d(440)} cx={86} cy={66} r={13} />
      <Check cx={86} cy={66} r={13} at={360} />
    </>
  );
}

function Announcements() {
  return (
    <>
      <g className="a-rise"><g className="a-float">
        <path className="sh" d="M43 39 73 25V71L43 57Z" />
        <rect className="pp" x={30} y={36} width={11} height={20} rx={2.5} />
        <path className="pp" d="M41 36 70 22V68L41 56Z" />
        <rect className="pp" x={68} y={20} width={5} height={50} rx={2.5} />
        <path className="pp" d="M45 57.5 48 69h7l-2.5-10" />
        <rect className="ln" x={47} y={42} width={14} height={2.5} rx={1.25} />
      </g></g>
      <path className="k a-draw" style={d(300)} pathLength={1} d="M81 35q6 10 0 20" />
      <path className="k a-draw" style={d(400)} pathLength={1} d="M89 29q10 16 0 32" />
      <g className="a-pop" style={d(560)}>
        <rect className="pp" x={76} y={70} width={30} height={10} rx={5} />
        <circle className="f3" cx={82} cy={75} r={1.8} />
        <rect className="ln" x={86} y={73.75} width={15} height={2.5} rx={1.25} />
      </g>
    </>
  );
}

function Notifications() {
  return (
    <>
      <rect className="ln" x={34} y={78} width={60} height={2.5} rx={1.25} />
      <g className="a-sway" style={d(80)}>
        <path className="sh" d="M67 25c-11 0-18 8-18 19v11l-5 8h46l-5-8V44c0-11-7-19-18-19z" />
        <circle className="pp" cx={64} cy={20} r={3} />
        <path className="pp" d="M58 63a6 6 0 0 0 12 0" />
        <path className="pp" d="M64 22c-11 0-18 8-18 19v11l-5 8h46l-5-8V41c0-11-7-19-18-19z" />
        <rect className="ln" x={52} y={48} width={24} height={2.5} rx={1.25} />
      </g>
      <Check cx={82} cy={28} r={9} at={520} />
    </>
  );
}

function Campaigns() {
  const ys = [24, 48, 72];
  return (
    <>
      <g className="a-rise">
        <Card x={20} y={26} w={36} h={46} />
        <rect className="t2" x={25} y={32} width={14} height={5} rx={2.5} />
        <rect className="kx" x={25} y={41} width={22} height={3} rx={1.5} />
        <Lines x={25} y={49} ws={[26, 20, 24]} />
      </g>
      {ys.map((y, i) => <path key={y} className="k a-draw" style={d(200 + i * 60)} pathLength={1} d={`M56 49C74 49 74 ${y} 88 ${y}`} />)}
      <g className="a-pop" style={d(400)}><Avatar cx={98} cy={24} r={9} tone={0} /></g>
      <g className="a-pop" style={d(460)}><Avatar cx={98} cy={48} r={9} tone={1} /></g>
      <circle className="dash a-pop" style={d(520)} cx={98} cy={72} r={9} />
      <Check cx={106} cy={31} r={4.5} at={600} />
      <Check cx={106} cy={55} r={4.5} at={660} />
    </>
  );
}

function People() {
  return (
    <>
      <g className="a-rise"><Avatar cx={42} cy={52} r={12} tone={0} /></g>
      <g className="a-rise" style={d(80)}><Avatar cx={84} cy={52} r={12} tone={1} /></g>
      <g className="a-rise" style={d(160)}><circle className="sh" cx={66} cy={50} r={16} /><circle className="pp" cx={63} cy={47} r={16} /><Avatar cx={63} cy={47} r={14} tone={3} /></g>
      <g className="a-pop" style={d(480)}>
        <circle className="dash" cx={106} cy={66} r={9} />
        <path className="k" opacity={0.6} d="M102.5 66h7M106 62.5v7" />
      </g>
    </>
  );
}

function Departments() {
  const node = (x: number, tone: Tone) => (
    <><Card x={x} y={58} w={24} h={16} r={4} /><circle className={`f${tone}`} cx={x + 6} cy={66} r={2} /><rect className="ln" x={x + 10} y={64.75} width={9} height={2.5} rx={1.25} /></>
  );
  return (
    <>
      <path className="k a-draw" style={d(200)} pathLength={1} d="M64 36v10M34 46h60M34 46v12M64 46v12M94 46v12" />
      <g className="a-rise">
        <Card x={50} y={20} w={28} h={16} r={4} />
        <circle className="f0" cx={56} cy={28} r={2} />
        <rect className="ln" x={60} y={26.75} width={13} height={2.5} rx={1.25} />
      </g>
      <g className="a-rise" style={d(380)}>{node(22, 1)}</g>
      <g className="a-rise" style={d(440)}>{node(52, 2)}</g>
      <g className="a-pop" style={d(540)}>
        <rect className="dash" x={82} y={58} width={24} height={16} rx={4} />
        <path className="k" opacity={0.6} d="M91 66h6M94 63v6" />
      </g>
    </>
  );
}

function Groups() {
  return (
    <>
      <g className="a-rise"><Card x={24} y={24} w={48} h={48} r={14} /></g>
      <rect className="dash a-rise" style={d(380)} x={56} y={30} width={48} height={46} rx={14} />
      <g className="a-pop" style={d(180)}><Avatar cx={38} cy={40} r={7} tone={0} /></g>
      <g className="a-pop" style={d(240)}><Avatar cx={54} cy={40} r={7} tone={3} /></g>
      <g className="a-pop" style={d(300)}><Avatar cx={42} cy={57} r={7} tone={1} /></g>
      <g className="a-pop" style={d(560)}><Avatar cx={64} cy={56} r={7} tone={4} /></g>
    </>
  );
}

function Roles() {
  return (
    <>
      <g className="a-rise">
        <Card x={30} y={20} w={48} h={58} r={6} />
        <rect className="ln" x={47} y={25} width={14} height={3} rx={1.5} />
        <Avatar cx={54} cy={43} r={10} tone={2} />
        <rect className="kx" x={42} y={59} width={24} height={3} rx={1.5} />
        <rect className="ln" x={45} y={66} width={18} height={2.5} rx={1.25} />
      </g>
      <g transform="translate(82 62) rotate(-32)"><g className="a-pop" style={d(400)}>
        <path className="pp" d="M8 -2.75H30V2.75H8Z" />
        <path className="k" d="M23 2.75v4M27.5 2.75v3" />
        <circle className="t5" r={9} /><circle className="k" r={9} />
        <circle className="pp" r={3} />
      </g></g>
    </>
  );
}

function Templates() {
  const card = (rot: number, c: string, at: number, body?: ReactNode) => (
    <g className="a-fan" style={{ ...d(at), transform: `rotate(${rot}deg)` }}>
      <Card x={46} y={22} w={36} h={50} c={c} />{body}
    </g>
  );
  return (
    <>
      {card(-14, 'ps', 0)}
      {card(14, 'ps', 80)}
      {card(0, 'pp', 160, <>
        <rect className="t2" x={50} y={26} width={28} height={9} rx={2} />
        <Lines x={50} y={41} ws={[24, 18, 26, 14]} />
        <rect className="dash" x={50} y={63} width={28} height={5} rx={2} />
      </>)}
    </>
  );
}

function Files() {
  return (
    <>
      <path className="sh" d="M31 35a4 4 0 0 1 4-4h16l5 6h43a4 4 0 0 1 4 4v34a4 4 0 0 1-4 4H35a4 4 0 0 1-4-4z" />
      <path className="ps" d="M28 32a4 4 0 0 1 4-4h16l5 6h43a4 4 0 0 1 4 4v34a4 4 0 0 1-4 4H32a4 4 0 0 1-4-4z" />
      <g className="a-rise" style={d(120)}>
        <rect className="pp" x={40} y={22} width={40} height={30} rx={3} />
        <Lines x={46} y={29} ws={[22, 28, 16]} />
      </g>
      <path className="pp" d="M28 44h72v28a4 4 0 0 1-4 4H32a4 4 0 0 1-4-4z" />
      <g className="a-pop" style={d(460)}>
        <path className="t1" d="M98 58l9 3.5v6.5c0 5.5-4 9-9 10.5-5-1.5-9-5-9-10.5v-6.5z" />
        <path className="k" d="M98 58l9 3.5v6.5c0 5.5-4 9-9 10.5-5-1.5-9-5-9-10.5v-6.5z" />
        <path className="s1 a-draw" style={d(600)} pathLength={1} d="M94.6 68l2.4 2.4 4.4-4.6" />
      </g>
    </>
  );
}

function Audit() {
  return (
    <>
      <path className="k a-draw" pathLength={1} d="M38 24v52" />
      <circle className="kx" cx={38} cy={29} r={3.5} />
      <g className="a-rise" style={d(120)}>
        <Card x={48} y={20} w={52} h={18} r={4} />
        <rect className="kx" x={53} y={25} width={14} height={3} rx={1.5} />
        <rect className="ln" x={53} y={31} width={30} height={2.5} rx={1.25} />
        <rect className="t1" x={86} y={24} width={9} height={5} rx={2} />
      </g>
      <g className="a-rise" style={d(260)}><circle className="pp" cx={38} cy={50} r={3} /><rect className="dash" x={48} y={43} width={52} height={14} rx={4} /></g>
      <g className="a-rise" style={d(360)}><circle className="pp" cx={38} cy={70} r={3} /><rect className="dash" x={48} y={63} width={52} height={14} rx={4} /></g>
    </>
  );
}

function NoMatch() {
  return (
    <>
      <g className="a-rise">
        <Card x={28} y={20} w={52} h={56} />
        {[32, 42, 52, 62].map((y, i) => <path key={y} className="dash" d={`M36 ${y}h${[30, 22, 34, 18][i]}`} />)}
      </g>
      <g className="a-rise" style={d(200)}><g className="a-float">
        <path className="k" style={{ strokeWidth: 3.5 }} d="M92 69l10 10" />
        <circle className="sh" cx={86} cy={61} r={13} />
        <circle className="pp" cx={83} cy={58} r={13} />
        <path className="k" opacity={0.45} d="M76 53.5a8 8 0 0 1 6-3.5" />
        <rect className="ln" x={78} y={57} width={10} height={2.5} rx={1.25} />
      </g></g>
    </>
  );
}

function Generic() {
  return (
    <>
      <rect className="dash a-rise" x={58} y={28} width={40} height={50} rx={5} />
      <g className="a-rise" style={d(120)}>
        <Card x={32} y={20} w={40} h={52} />
        <Lines x={38} y={30} ws={[22, 28, 18]} />
        <rect className="dash" x={38} y={52} width={28} height={12} rx={3} />
      </g>
      <circle className="rd a-pop" style={d(460)} cx={66} cy={27} r={2} />
    </>
  );
}

const ART: Record<ArtKind, { tone: Tone; draw: () => ReactNode }> = {
  library: { tone: 0, draw: Library },
  'caught-up': { tone: 1, draw: CaughtUp },
  announcements: { tone: 3, draw: Announcements },
  notifications: { tone: 5, draw: Notifications },
  campaigns: { tone: 2, draw: Campaigns },
  people: { tone: 4, draw: People },
  departments: { tone: 0, draw: Departments },
  groups: { tone: 2, draw: Groups },
  roles: { tone: 5, draw: Roles },
  templates: { tone: 3, draw: Templates },
  files: { tone: 0, draw: Files },
  audit: { tone: 1, draw: Audit },
  'no-match': { tone: 6, draw: NoMatch },
  generic: { tone: 6, draw: Generic },
};

export const ART_KINDS = Object.keys(ART) as ArtKind[];

/** The illustration for one kind of empty state, on its pastel plate. Hidden from assistive tech. */
export function Art({ kind }: { kind: ArtKind }) {
  const { tone, draw: Draw } = ART[kind] ?? ART.generic;
  return (
    <svg className={`art art-${kind}`} viewBox="0 0 128 96" aria-hidden="true" focusable="false">
      <circle className={`t${tone} pl`} cx={64} cy={48} r={40} />
      <Draw />
    </svg>
  );
}
