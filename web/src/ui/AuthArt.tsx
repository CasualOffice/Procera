import { type CSSProperties, useEffect, useState } from 'react';
import './illustrations/illustrations.css';
import './auth-art.css';

/**
 * The sign-in side panel: Procera's loop told in four beats, in the empty states' drawing language
 * (ink outlines, paper cards with a hard offset shadow, tag pastels). A policy is drafted, reviewed,
 * approved with a signature, then published and acknowledged, and each beat adds a link to the
 * audit chain. Decorative only (aria-hidden): the page's own text carries the meaning. It pauses
 * while the tab is hidden, and reduced motion shows the last beat without moving.
 */
const BEATS = [
  { kicker: 'Draft', title: 'Write the policy once', body: 'Policies and procedures live in one library, with every version kept.' },
  { kicker: 'Review', title: 'Route it to the right people', body: 'Reviewers are picked by role, department or manager, never the author.' },
  { kicker: 'Approve', title: 'Approve with a signature', body: 'Each decision is signed and stamped, with separation of duties built in.' },
  { kicker: 'Publish', title: 'Everyone acknowledges', body: 'Publish to the people it applies to and see who has read it.' },
] as const;

const BEAT_MS = 3600;
const d = (ms: number) => ({ '--d': `${ms}ms` }) as CSSProperties;

function usePhase(): number {
  const reduced = typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
  const [phase, setPhase] = useState(reduced ? 3 : 0);
  useEffect(() => {
    if (reduced) return;
    let timer: number | undefined;
    const start = () => { timer = window.setInterval(() => setPhase((p) => (p + 1) % BEATS.length), BEAT_MS); };
    const onVis = () => { window.clearInterval(timer); if (!document.hidden) start(); };
    start();
    document.addEventListener('visibilitychange', onVis);
    return () => { window.clearInterval(timer); document.removeEventListener('visibilitychange', onVis); };
  }, [reduced]);
  return phase;
}

const TAGS = [
  { label: 'Draft', tone: 5, w: 44 },
  { label: 'In review', tone: 0, w: 62 },
  { label: 'Approved', tone: 1, w: 62 },
  { label: 'Published', tone: 2, w: 66 },
] as const;

function Person({ cx, cy, tone, on, check, at }: { cx: number; cy: number; tone: number; on: boolean; check: boolean; at: number }) {
  return (
    <g className={`aa-in ${on ? 'is-on' : ''}`} style={d(at)}>
      <circle className="sh" cx={cx + 2.5} cy={cy + 2.5} r={17} />
      <circle className={`t${tone}`} cx={cx} cy={cy} r={17} />
      <circle className="k" cx={cx} cy={cy} r={17} />
      <circle className="k" cx={cx} cy={cy - 4} r={5.5} />
      <path className="k" d={`M${cx - 9} ${cy + 11}a9 8 0 0 1 18 0`} />
      <g className={`aa-pop ${check ? 'is-on' : ''}`} style={d(at + 260)}>
        <circle className="t1" cx={cx + 13} cy={cy + 12} r={7} />
        <circle className="k" cx={cx + 13} cy={cy + 12} r={7} />
        <path className="s1" style={{ strokeWidth: 2 }} d={`M${cx + 9.8} ${cy + 12.2}l2.2 2.2 4.4-4.6`} />
      </g>
    </g>
  );
}

export function AuthArt() {
  const phase = usePhase();
  const audience = [0, 1, 2, 3, 4, 5];
  return (
    <aside className="auth-art" aria-hidden="true">
      <div className="auth-art-stage">
        <svg className="art aa" viewBox="0 0 400 330" focusable="false">
          {/* The document */}
          <g className="a-rise"><g className="aa-float">
            <rect className="sh" x={44} y={34} width={176} height={222} rx={10} />
            <rect className="pp" x={40} y={30} width={176} height={222} rx={10} />
            {TAGS.map((t, i) => (
              <g key={t.label} className={`aa-swap ${phase === i ? 'is-on' : ''}`}>
                <rect className={`t${t.tone}`} x={58} y={48} width={t.w} height={18} rx={9} />
                <circle className={`f${t.tone}`} cx={68} cy={57} r={3} />
                <text className={`aa-tag f${t.tone}`} x={75} y={61} fontSize={9.5}>{t.label}</text>
              </g>
            ))}
            <rect className="kx" x={58} y={80} width={112} height={7} rx={3.5} />
            {[100, 112, 124, 136, 148].map((y, i) => <rect key={y} className="ln" x={58} y={y} width={[140, 128, 136, 96, 120][i]} height={4} rx={2} />)}
            <rect className={`aa-caret ${phase === 0 ? 'is-on' : ''}`} x={180} y={145} width={2} height={10} rx={1} />
            {[170, 182].map((y, i) => <rect key={y} className="ln" x={58} y={y} width={[132, 84][i]} height={4} rx={2} />)}
            {/* Signature */}
            <path className="k" opacity={0.35} d="M58 228h96" />
            <path className={`aa-sign ${phase >= 2 ? 'is-on' : ''}`} pathLength={1}
              d="M62 222c6-14 10-16 11-8s-3 12 2 5 8-14 10-6-2 10 4 4 9-8 12-3 5 4 10-2 8-5 14-1" />
            <g className={`aa-pop ${phase >= 3 ? 'is-on' : ''}`} style={d(120)}>
              <rect className="t2" x={164} y={206} width={38} height={30} rx={6} transform="rotate(-8 183 221)" />
              <rect className="k" x={164} y={206} width={38} height={30} rx={6} transform="rotate(-8 183 221)" />
              <text className="aa-stamp f2" x={170} y={226} fontSize={11} transform="rotate(-8 183 221)">v1.0</text>
            </g>
          </g></g>

          {/* Reviewers */}
          {[78, 140, 202].map((y, i) => (
            <path key={y} className={`dash aa-in ${phase >= 1 ? 'is-on' : ''}`} style={d(i * 120)} d={`M222 ${141 + (i - 1) * 20}C252 ${141 + (i - 1) * 20} 256 ${y} 288 ${y}`} />
          ))}
          {[0, 2, 3].map((tone, i) => (
            <Person key={tone} cx={312} cy={78 + i * 62} tone={tone} on={phase >= 1} check={phase >= 2} at={i * 140} />
          ))}

          {/* Audience acknowledging */}
          <g className={`aa-in ${phase >= 3 ? 'is-on' : ''}`}>
            {audience.map((n) => (
              <g key={n} className={`aa-pop ${phase >= 3 ? 'is-on' : ''}`} style={d(160 + n * 110)}>
                <circle className={`t${(n % 5) + 1}`} cx={58 + n * 26} cy={280} r={10} />
                <circle className="k" cx={58 + n * 26} cy={280} r={10} />
                <path className="s1" style={{ strokeWidth: 1.75 }} d={`M${54 + n * 26} ${280}l2.5 2.5 4.5-5`} />
              </g>
            ))}
          </g>

          {/* Audit chain: one link per beat */}
          <text className="aa-label" x={246} y={296} fontSize={9}>Audit chain</text>
          {[0, 1, 2, 3].map((n) => (
            <g key={n}>
              {n > 0 && <path className={`aa-link ${phase >= n ? 'is-on' : ''}`} d={`M${246 + (n - 1) * 38 + 26} 312h12`} />}
              <rect className={`aa-block ${phase >= n ? 'is-on' : ''}`} x={246 + n * 38} y={304} width={26} height={16} rx={4} />
              <rect className="k" x={246 + n * 38} y={304} width={26} height={16} rx={4} />
            </g>
          ))}
        </svg>
      </div>
      <div className="auth-art-copy">
        <ol className="auth-art-steps">
          {BEATS.map((b, i) => <li key={b.kicker} className={phase === i ? 'is-on' : phase > i ? 'is-done' : ''}>{b.kicker}</li>)}
        </ol>
        <div className="auth-art-captions">
          {BEATS.map((b, i) => (
            <div key={b.kicker} className={`auth-art-caption ${phase === i ? 'is-on' : ''}`}>
              <p className="auth-art-title">{b.title}</p>
              <p className="auth-art-body">{b.body}</p>
            </div>
          ))}
        </div>
        <p className="auth-art-chain"><span aria-hidden="true" className="auth-art-dot" /> Every change lands on a tamper-evident audit chain.</p>
      </div>
    </aside>
  );
}
