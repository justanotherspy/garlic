/**
 * Today's figures as `garlic status --json` reports them, in minutes. Every
 * figure is a union over all sessions, so two agents running through the same
 * hour are one hour. `youOnly + agentsOnly + both` is `total`.
 */
export type GarlicStatus = {
  total: number
  /** Time you were reading, thinking or typing, in any session. */
  you: number
  /** Time at least one agent was generating. */
  agents: number
  youOnly: number
  agentsOnly: number
  both: number
  /** Time two or more sessions were active at once. */
  overlap: number
  agentsRunning: number
  thresholds: number[]
  given: number[]
  next: number | null
  isPaused: boolean
  /** When these figures were read, in `$.clock.now()` milliseconds. */
  asOf: number
}

export type GarlicNudge = {
  message: string
  kind: 'threshold' | 'final' | 'bedtime'
  /** When it fired, in `$.clock.now()` milliseconds. */
  at: number
}

/** What the panel shows: today, a report from the CLI, or the reset question. */
export type GarlicView =
  | { kind: 'today' }
  | { kind: 'report'; title: string; text: string }
  | { kind: 'confirm-reset' }

declare module 'claude-code' {
  interface PluginState {
    garlic: {
      status: Shaped<GarlicStatus | null>
      nudge: Shaped<GarlicNudge | null>
      view: Shaped<GarlicView>
      /** Why garlic is not tracking, in one line; null while it is. */
      problem: string | null
      /** True while garlic's settings hooks are installed beside the plugin. */
      hasSettingsHooks: boolean
    }
  }
}
