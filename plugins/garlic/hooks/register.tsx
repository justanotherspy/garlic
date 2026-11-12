// garlic as a Claude Code mod.
//
// The module does two jobs. It tracks: the session's start, each turn's start
// and end, and the session's end are handed to the `garlic` binary, which owns
// the accounting (the same engine the settings hooks drive, so the CLI, the
// status line and the optional shared backend all agree). And it shows: the
// day's time on the status line, a break nudge above the prompt when one
// fires, and a small panel (`/garlic`) with the split between your time and
// your agents' time and the buttons to pause nudges or reset the day.
//
// A nudge is shown to the person directly. Nothing is added to the prompt and
// the model is never asked to relay anything.

import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, Timer } from 'claude-code'

import {
  GARLIC,
  INSTALL_HINT,
  MIN_CLI,
  VAMPIRE,
  bar,
  dayTarget,
  duration,
  legacyNudge,
  parseAsk,
  parseReply,
  shares,
  statusLine,
  toStatus,
} from './garlic'
import type { HookReply } from './garlic'
import type { GarlicView } from '../types'

const PANE = 'garlic'
const TICK_MS = 60_000
// A nudge leaves the band by itself after this long, dismissed or not.
const NUDGE_SHOWN_MS = 15 * 60_000
const TODAY: GarlicView = { kind: 'today' }

const status = atom({ plugin: 'garlic', key: 'status' } as const, null, {
  shape: 'status-1',
})
const nudge = atom({ plugin: 'garlic', key: 'nudge' } as const, null, {
  shape: 'nudge-1',
})
const view = atom({ plugin: 'garlic', key: 'view' } as const, TODAY, {
  shape: 'view-1',
})
const problem = atom({ plugin: 'garlic', key: 'problem' } as const, null)
const hasSettingsHooks = atom(
  { plugin: 'garlic', key: 'hasSettingsHooks' } as const,
  false,
)

type Engine = EngineInterface
type HookEvent = 'session-start' | 'prompt' | 'stop' | 'session-end' | 'tick'

// The module's own state. It starts over on a reload, which is right: a
// reload is the moment to look for the binary again.
let showsStatusLine = true
// Set once the binary turned out to be missing or too old: every later event
// is then a no-op, so a session without garlic pays for one spawn.
let isOff = false
// Set once the binary turned out to predate `hook --json`: it still tracks,
// through the plain hook commands, without live totals or mid-turn nudges.
let isLegacy = false
let timer: Timer | undefined

/** Runs `garlic <argv>`; null when the binary cannot be started. */
const garlic = async ($: Engine, argv: string[], stdin?: string) => {
  try {
    return await $.process.run(['garlic', ...argv], {
      timeoutMs: 5_000,
      ...(stdin === undefined ? {} : { stdin }),
    })
  } catch {
    return null
  }
}

/** Says one thing about the install, once a day at most. */
const tell = async ($: Engine, text: string) => {
  const today = new Date(await $.clock.now()).toISOString().slice(0, 10)

  if ((await $.store.get('toldOn')) !== today) {
    await $.store.set('toldOn', today)
    $.ui.toast(`${GARLIC} ${text}`, { timeoutMs: 12_000 })
  }
}

/** Stops tracking for this session and says why. */
const turnOff = async ($: Engine, why: string) => {
  isOff = true
  timer?.cancel()
  $.ui.status(undefined)
  await update($, problem, () => why)
  await tell($, why)
}

const show = async ($: Engine, reply: HookReply) => {
  if (reply.status !== null) {
    const fresh = reply.status
    await update($, status, () => fresh)

    if (showsStatusLine) {
      $.ui.status(statusLine(fresh))
    }
  }

  if (reply.hasSettingsHooks !== undefined) {
    const has = reply.hasSettingsHooks
    await update($, hasSettingsHooks, () => has)
  }

  if (reply.nudge !== null) {
    const fired = reply.nudge
    await update($, nudge, () => fired)
    $.ui.toast(`${GARLIC} ${fired.message}`, { timeoutMs: 15_000 })
  }
}

/**
 * The same event for a garlic from before `hook --json`: the plain hook
 * command books the time, and `status --json` gives the readout. That garlic
 * has no `tick`, so its totals move only when a turn starts or ends.
 */
const trackLegacy = async ($: Engine, event: HookEvent, stdin: string) => {
  if (event === 'tick') {
    return
  }

  const ran = await garlic($, ['hook', event], stdin)
  const seen = await garlic($, ['status', '--json'])
  const now = await $.clock.now()
  let parsed: unknown = null

  try {
    parsed = JSON.parse(seen?.stdout ?? '')
  } catch {
    // An unreadable status leaves the last readout standing.
  }

  await show($, {
    status: toStatus(parsed, now),
    nudge: ran === null ? null : legacyNudge(ran.stdout, now),
  })
}

/**
 * Hands one event to the binary and shows what it answers. Never throws: a
 * session must not be worse off for having garlic installed.
 */
const track = async ($: Engine, event: HookEvent) => {
  if (isOff) {
    return
  }

  try {
    const stdin = JSON.stringify({ session_id: await $.session.id() })

    if (isLegacy) {
      await trackLegacy($, event, stdin)

      return
    }

    const ran = await garlic($, ['hook', event, '--json'], stdin)

    if (ran === null) {
      await turnOff(
        $,
        `garlic is not installed, so time is not being tracked. ${INSTALL_HINT}`,
      )

      return
    }

    // clap exits 2 on a flag it does not know: a garlic from before --json.
    if (ran.exitCode === 2) {
      isLegacy = true
      timer?.cancel()
      await tell(
        $,
        `garlic ${MIN_CLI} or newer adds live totals and nudges while a turn runs. Upgrade: brew upgrade --cask garlic (or: cargo install garlic-ward --force)`,
      )
      await trackLegacy($, event, stdin)

      return
    }

    const reply =
      ran.exitCode === 0 ? parseReply(ran.stdout, await $.clock.now()) : null

    if (reply !== null) {
      await show($, reply)
    }
  } catch {
    // Leave the last good readout standing.
  }
}

const openPanel = ($: Engine) =>
  $.ui.open({
    id: PANE,
    title: 'garlic',
    focus: true,
    closeOnEscape: true,
    rows: 10,
  })

const togglePause = async ($: Engine) => {
  const ran = await garlic($, ['ignore'])
  await update($, nudge, () => null)
  await track($, 'tick')

  return ran?.stdout.trim() ?? ''
}

const report = async ($: Engine, title: string, argv: string[]) => {
  const ran = await garlic($, argv)
  const text =
    ran !== null && ran.exitCode === 0
      ? ran.stdout.trimEnd()
      : 'garlic could not produce this report.'

  return { title, text }
}

export const register: Register = (on, options) => {
  showsStatusLine = options.status_line !== false

  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'garlic',
      description:
        "Open garlic: today's time in Claude Code, you vs your agents, and break nudges",
      argumentHint: '[week|month|pause|reset]',
    })
    await track($, 'session-start')

    if (!isOff) {
      // Time passes between events: a long turn crosses a nudge line while it
      // runs, and the readout should move while nothing else does.
      timer = $.clock.every(TICK_MS, () => {
        void track($, 'tick')
      })
    }

    return next(e)
  })

  // A turn of the main loop is one agent interval: prompt to stop. Subagent
  // turns run inside it (or after it, in the background) and are not events
  // of the person's.
  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      await track($, 'prompt')
    }

    return next(e)
  })

  // Every end counts, an interrupt included: the settings Stop hook does not
  // run on an interrupt, which left the time after one booked as generation.
  on('turn.complete', async ($, e, next) => {
    const done = await next(e)

    if (e.agentId === undefined) {
      await track($, 'stop')
    }

    return done
  })

  on('session.end', async ($, e, next) => {
    timer?.cancel()
    await track($, 'session-end')

    return next(e)
  })

  on('command.run', { command: 'garlic' }, async ($, e) => {
    const ask = parseAsk(e.args ?? '')
    const why = await read($, problem)

    if (why !== null) {
      return { text: `garlic: ${why}` }
    }

    switch (ask.kind) {
      case 'pause':
        return { text: await togglePause($) }
      case 'report': {
        const { text } = await report($, ask.title, ask.argv)

        return { text }
      }
      case 'unknown':
        return {
          text: 'garlic: try /garlic, /garlic week, /garlic month, /garlic pause or /garlic reset',
        }
      case 'reset':
      case 'panel': {
        await track($, 'tick')
        // A reset always goes through the panel's own question.
        const first: GarlicView =
          ask.kind === 'reset' ? { kind: 'confirm-reset' } : TODAY
        await update($, view, () => first)
        const opened = await openPanel($)

        if (opened.isPlaced) {
          return {}
        }

        const now = await read($, status)

        return {
          text:
            now === null
              ? 'garlic: nothing tracked yet today.'
              : statusLine(now),
        }
      }
    }
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Button, Text } = $.ui.resolve(e)
    const now = await read($, status)
    const showing = await read($, view)
    const why = await read($, problem)
    const columns = Math.max(24, e.props.bodyColumns)
    const close = () => $.ui.close({ id: PANE })
    const back = () => update($, view, () => TODAY)

    if (why !== null || now === null) {
      return (
        <Box flexDirection="column">
          <Text>{why ?? 'Nothing tracked yet today.'}</Text>
          <Box>
            <Button key="close" label="Close" role="dismiss" onPress={close} />
          </Box>
        </Box>
      )
    }

    if (showing.kind === 'confirm-reset') {
      return (
        <Box flexDirection="column">
          <Text>
            Reset today to zero? This clears {duration(now.total)} of tracked
            time and today's nudges.
          </Text>
          <Box gap={1}>
            <Button
              key="reset-yes"
              label="Yes, reset today"
              onPress={async () => {
                await garlic($, ['reset', '-y'])
                await update($, nudge, () => null)
                await track($, 'tick')
                await back()
              }}
            />
            <Button
              key="reset-no"
              label="Cancel"
              variant="primary"
              autoFocus
              onPress={back}
            />
          </Box>
        </Box>
      )
    }

    if (showing.kind === 'report') {
      return (
        <Box flexDirection="column">
          <Text bold>{showing.title}</Text>
          {showing.text.split('\n').map(line => (
            <Text>{line === '' ? ' ' : line}</Text>
          ))}
          <Box gap={1}>
            <Button key="back" label="Back" variant="primary" onPress={back} />
            <Button key="close" label="Close" role="dismiss" onPress={close} />
          </Box>
        </Box>
      )
    }

    const target = dayTarget(now)
    const width = Math.min(40, columns - 2)
    const split = shares(now, width)
    const isOver = target !== null && now.total >= target
    const next = now.isPaused
      ? 'Nudges are paused for today. Tracking continues.'
      : now.next === null
        ? target === null
          ? 'No nudge thresholds are set.'
          : "You've crossed every threshold today. Time to stop."
        : `Next nudge at ${duration(now.next)}, ${duration(Math.max(0, now.next - now.total))} to go.`
    const running =
      now.agentsRunning === 0
        ? ''
        : now.agentsRunning === 1
          ? ' · 1 agent running now'
          : ` · ${now.agentsRunning} agents running now`
    const showReport = (title: string, argv: string[]) => async () => {
      const made = await report($, title, argv)
      await update($, view, (): GarlicView => ({ kind: 'report', ...made }))
    }

    return (
      <Box flexDirection="column">
        <Text bold>
          {VAMPIRE} {duration(now.total)} in Claude Code today
          {target === null ? '' : ` of ${duration(target)}`}
        </Text>
        {target !== null && (
          <Text color={isOver ? 'error' : 'claude'}>
            {bar(now.total / target, width)}
          </Text>
        )}
        <Text dimColor>{next}</Text>
        <Text> </Text>
        <Text>
          <Text color="success">{'█'.repeat(split.youOnly)}</Text>
          <Text color="warning">{'█'.repeat(split.both)}</Text>
          <Text color="suggestion">{'█'.repeat(split.agentsOnly)}</Text>
        </Text>
        <Text>
          <Text color="success">█</Text> only you {duration(now.youOnly)}
          {'  '}
          <Text color="warning">█</Text> both {duration(now.both)}
          {'  '}
          <Text color="suggestion">█</Text> only agents{' '}
          {duration(now.agentsOnly)}
        </Text>
        <Text dimColor>
          you {duration(now.you)} · agents {duration(now.agents)}
          {now.overlap > 0
            ? ` · ${duration(now.overlap)} with 2+ sessions at once`
            : ''}
          {running}
        </Text>
        <Text> </Text>
        <Box gap={1} flexWrap="wrap">
          <Button
            key="pause"
            hotkey="p"
            label={now.isPaused ? 'Resume nudges' : 'Pause nudges today'}
            onPress={async () => {
              await togglePause($)
            }}
          />
          <Button
            key="reset"
            hotkey="r"
            label="Reset today"
            onPress={() =>
              update($, view, (): GarlicView => ({ kind: 'confirm-reset' }))
            }
          />
          <Button
            key="week"
            hotkey="w"
            label="Week"
            onPress={showReport('This week', ['status', '--week'])}
          />
          <Button
            key="month"
            hotkey="m"
            label="Month"
            onPress={showReport('This month', ['status', '--month'])}
          />
          <Button key="close" label="Close" role="dismiss" onPress={close} />
        </Box>
      </Box>
    )
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) {
      return next(e)
    }

    const fired = await read($, nudge)
    const now = await read($, status)
    const isFresh =
      fired !== null && (now === null || now.asOf - fired.at < NUDGE_SHOWN_MS)

    if (fired !== null && isFresh) {
      const { Box, Button, Text } = $.ui.resolve(e)
      const dismiss = () => update($, nudge, () => null)

      return (
        <Box flexDirection="column">
          <Text color={fired.kind === 'final' ? 'error' : 'warning'}>
            {GARLIC} {fired.message}
          </Text>
          <Box gap={1}>
            <Button
              key="nudge-pause"
              label="Pause nudges today"
              onPress={async () => {
                await togglePause($)
              }}
            />
            <Button
              key="nudge-open"
              label="Open garlic"
              onPress={async () => {
                await update($, view, () => TODAY)
                await openPanel($)
              }}
            />
            <Button
              key="nudge-dismiss"
              label="Dismiss"
              role="dismiss"
              onPress={dismiss}
            />
          </Box>
        </Box>
      )
    }

    if (await read($, hasSettingsHooks)) {
      const { Box, Button, Text } = $.ui.resolve(e)
      const hide = () => update($, hasSettingsHooks, () => false)

      return (
        <Box flexDirection="column">
          <Text dimColor>
            {GARLIC} garlic's hooks are still in ~/.claude/settings.json. The
            plugin replaces them.
          </Text>
          <Box gap={1}>
            <Button
              key="hooks-remove"
              label="Remove them"
              onPress={async () => {
                await garlic($, ['setup', '--remove-hooks'])
                await hide()
              }}
            />
            <Button
              key="hooks-keep"
              label="Not now"
              role="dismiss"
              onPress={hide}
            />
          </Box>
        </Box>
      )
    }

    return next(e)
  })
}
