// Pure helpers for the garlic hooks module: reading what the CLI prints and
// formatting it. Nothing here touches the engine, so it is all unit-testable.

import type { GarlicNudge, GarlicStatus } from '../types'

export const GARLIC = '\u{1F9C4}'
export const VAMPIRE = '\u{1F9DB}'

/** The oldest garlic that understands `garlic hook <event> --json`. */
export const MIN_CLI = '0.4.0'

export const INSTALL_HINT =
  'brew install --cask justanotherspy/tap/garlic (or: cargo install garlic-ward)'

/** Minutes as the CLI prints them: `45m`, `1h 05m`. Floors, as the CLI does. */
export function duration(minutes: number): string {
  const whole = Math.max(0, Math.floor(minutes))
  const hours = Math.floor(whole / 60)
  const rest = whole % 60

  return hours > 0 ? `${hours}h ${String(rest).padStart(2, '0')}m` : `${rest}m`
}

const number = (value: unknown): number =>
  typeof value === 'number' && Number.isFinite(value) ? value : 0

const numbers = (value: unknown): number[] =>
  Array.isArray(value) ? value.filter(n => typeof n === 'number') : []

const record = (value: unknown): Record<string, unknown> | undefined =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined

/** The `status` object of `garlic hook --json` / `garlic status --json`. */
export function toStatus(raw: unknown, asOf: number): GarlicStatus | null {
  const status = record(raw)

  if (status === undefined || typeof status.accumulated_minutes !== 'number') {
    return null
  }

  const next = status.next_threshold
  const total = number(status.accumulated_minutes)
  const you = number(status.user_minutes)
  const agents = number(status.agent_minutes)
  // A garlic from before the three-way split printed only the two unions:
  // the split follows from them and the total.
  const both =
    typeof status.both_minutes === 'number'
      ? status.both_minutes
      : Math.max(0, you + agents - total)

  return {
    total,
    you,
    agents,
    youOnly:
      typeof status.user_only_minutes === 'number'
        ? status.user_only_minutes
        : Math.max(0, you - both),
    agentsOnly:
      typeof status.agent_only_minutes === 'number'
        ? status.agent_only_minutes
        : Math.max(0, agents - both),
    both,
    overlap: number(status.overlap_minutes),
    agentsRunning: number(status.agents_running),
    thresholds: numbers(status.thresholds),
    given: numbers(status.nudges_given),
    next: typeof next === 'number' ? next : null,
    isPaused: status.ignored === true,
    asOf,
  }
}

/** The `nudge` object of `garlic hook --json`, or null when none fired. */
export function toNudge(raw: unknown, at: number): GarlicNudge | null {
  const nudge = record(raw)

  if (nudge === undefined || typeof nudge.message !== 'string') {
    return null
  }

  const kind =
    nudge.kind === 'final' || nudge.kind === 'bedtime'
      ? nudge.kind
      : 'threshold'

  return { message: nudge.message, kind, at }
}

/** What one `garlic hook <event> --json` run printed. */
export type HookReply = {
  status: GarlicStatus | null
  nudge: GarlicNudge | null
  /** Present on `session-start` only. */
  hasSettingsHooks?: boolean
}

export function parseReply(stdout: string, now: number): HookReply | null {
  let parsed: unknown

  try {
    parsed = JSON.parse(stdout)
  } catch {
    return null
  }

  const reply = record(parsed)

  if (reply === undefined) {
    return null
  }

  return {
    status: toStatus(reply.status, now),
    nudge: toNudge(reply.nudge, now),
    ...(typeof reply.classic_hooks === 'boolean'
      ? { hasSettingsHooks: reply.classic_hooks }
      : {}),
  }
}

/**
 * The nudge a garlic from before `--json` printed for the model to relay: the
 * settings hook's envelope, with the message quoted at the end of its frame.
 */
export function legacyNudge(stdout: string, at: number): GarlicNudge | null {
  let parsed: unknown

  try {
    parsed = JSON.parse(stdout)
  } catch {
    return null
  }

  const framed = record(record(parsed)?.hookSpecificOutput)?.additionalContext

  if (typeof framed !== 'string' || framed === '') {
    return null
  }

  const quoted = /Message to relay: "([\s\S]*)"\s*$/.exec(framed)

  return { message: quoted?.[1] ?? framed, kind: 'threshold', at }
}

/** The day's last threshold: the "session over" line, and the bar's full width. */
export function dayTarget(status: GarlicStatus): number | null {
  return status.thresholds.length > 0 ? Math.max(...status.thresholds) : null
}

/** The one-line readout: the status line, and the panel's heading. */
export function statusLine(status: GarlicStatus): string {
  const target = dayTarget(status)
  const parts = [
    `${VAMPIRE} ${duration(status.total)}${target === null ? '' : ` / ${duration(target)}`}`,
  ]

  if (status.you > 0 || status.agents > 0) {
    parts.push(
      `you ${duration(status.you)}`,
      `agents ${duration(status.agents)}`,
    )
  }

  if (status.agentsRunning > 1) {
    parts.push(`${status.agentsRunning} running`)
  }

  if (status.isPaused) {
    parts.push('nudges paused')
  }

  return parts.join(' · ')
}

/** A bar of `width` cells, `fraction` of it filled. */
export function bar(fraction: number, width: number): string {
  const cells = Math.max(1, Math.floor(width))
  const filled = Math.round(Math.min(1, Math.max(0, fraction)) * cells)

  return '█'.repeat(filled) + '░'.repeat(cells - filled)
}

/**
 * The total split three ways, as cell counts that add up to `width`: only you,
 * you and agents at once, only agents. Each non-zero share gets a cell.
 */
export function shares(
  status: GarlicStatus,
  width: number,
): { youOnly: number; both: number; agentsOnly: number } {
  const cells = Math.max(3, Math.floor(width))
  const parts = [status.youOnly, status.both, status.agentsOnly]
  const sum = parts.reduce((a, b) => a + b, 0)

  if (sum <= 0) {
    return { youOnly: 0, both: 0, agentsOnly: 0 }
  }

  const sized = parts.map(part =>
    part <= 0 ? 0 : Math.max(1, Math.round((part / sum) * cells)),
  )
  // Rounding and the one-cell floor can miss the width by a cell or two: the
  // widest share absorbs the difference.
  const widest = sized.indexOf(Math.max(...sized))
  sized[widest] =
    (sized[widest] ?? 0) + cells - sized.reduce((a, b) => a + b, 0)

  return {
    youOnly: sized[0] ?? 0,
    both: sized[1] ?? 0,
    agentsOnly: sized[2] ?? 0,
  }
}

/** `0.3.5` against `0.4.0`: true when `version` is at least `floor`. */
export function isAtLeast(version: string, floor: string): boolean {
  const parse = (text: string) =>
    text.split('.').map(part => Number.parseInt(part, 10) || 0)
  const [a, b] = [parse(version), parse(floor)]

  for (let i = 0; i < 3; i += 1) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0)

    if (diff !== 0) {
      return diff > 0
    }
  }

  return true
}

/** What `/garlic <args>` asks for. */
export type Ask =
  | { kind: 'panel' }
  | { kind: 'pause' }
  | { kind: 'reset' }
  | { kind: 'report'; title: string; argv: string[] }
  | { kind: 'unknown' }

export function parseAsk(args: string): Ask {
  const words = args.trim().toLowerCase().split(/\s+/).filter(Boolean)
  const key = words.filter(word => word !== 'status').join(' ')

  switch (key) {
    case '':
      return words.length === 0
        ? { kind: 'panel' }
        : { kind: 'report', title: 'Today', argv: ['status'] }
    case 'pause':
    case 'resume':
    case 'ignore':
      return { kind: 'pause' }
    case 'reset':
      return { kind: 'reset' }
    case 'week':
    case '--week':
      return { kind: 'report', title: 'This week', argv: ['status', '--week'] }
    case 'month':
    case '--month':
      return {
        kind: 'report',
        title: 'This month',
        argv: ['status', '--month'],
      }
    default:
      return { kind: 'unknown' }
  }
}
