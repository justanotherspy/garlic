import { expect, mock, test } from 'claude-code/testing'
import type { CommandRunInput, On } from 'claude-code'

import {
  bar,
  duration,
  isAtLeast,
  parseAsk,
  parseReply,
  shares,
  statusLine,
  toStatus,
} from '../hooks/garlic'

const BAND = {
  plugin: 'garlic',
  component: 'AbovePrompt',
  props: {
    hasSurvey: false,
    isCollapsed: false,
    isWorking: false,
    maxRows: 10,
    bodyColumns: 80,
    scroll: { offset: 0, bodyRows: 9 },
    view: {},
  },
} as const

const PANE = {
  plugin: 'garlic',
  component: 'Pane',
  requestId: 'garlic',
  props: {
    title: 'garlic',
    isFocused: true,
    bodyColumns: 60,
    placement: 'inline',
    scroll: { offset: 0, bodyRows: 12 },
    view: {},
  },
} as const

// 2h 15m today: 45m only you, 15m both, 1h 15m only agents.
const STATUS = {
  accumulated_minutes: 135,
  agent_minutes: 90,
  user_minutes: 60,
  overlap_minutes: 12,
  user_only_minutes: 45,
  agent_only_minutes: 75,
  both_minutes: 15,
  agents_running: 1,
  thresholds: [60, 120, 180, 240],
  nudges_given: [60, 120],
  next_threshold: 180,
  ignored: false,
  date: '2026-11-12',
}

type Fake = {
  /** Every `garlic` argv the plugin ran, the binary's name dropped. */
  calls: string[][]
  stdin: (string | undefined)[]
  status: typeof STATUS
  nudge: { message: string; kind: string; threshold_minutes: number } | null
  hasSettingsHooks: boolean
  toasts: string[]
  statusLines: (string | undefined)[]
  closed: number
}

/** Stands for the engine and for the `garlic` binary beneath the plugin. */
function world(on: On, exists = true, isOld = false): Fake {
  const fake: Fake = {
    calls: [],
    stdin: [],
    status: { ...STATUS },
    nudge: null,
    hasSettingsHooks: false,
    toasts: [],
    statusLines: [],
    closed: 0,
  }

  mock.store(on)
  on('session.id', () => ({ value: 'session-1' }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.end', (_, e) => ({ sessionId: e.sessionId }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  on('command.register', (_, e) => ({ value: { command: e.name } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('ui.close', () => {
    fake.closed += 1

    return { value: undefined }
  })
  on('ui.toast', (_, e) => {
    fake.toasts.push(e.text)

    return { value: undefined }
  })
  on('ui.status', (_, e) => {
    fake.statusLines.push(e.text)

    return { value: undefined }
  })
  on('process.run', (_, e) => {
    if (!exists) {
      throw new Error('spawn garlic ENOENT')
    }

    const argv = e.argv.slice(1)
    fake.calls.push(argv)
    fake.stdin.push(e.init?.stdin)
    let stdout = ''

    // A garlic from before `hook --json`: clap refuses the flag and the tick
    // event, the prompt hook prints the envelope the model used to relay, and
    // `status --json` has the two unions but not the three-way split.
    if (isOld) {
      const isUnknown = argv.includes('--json') && argv[0] === 'hook'
      const {
        user_only_minutes,
        agent_only_minutes,
        both_minutes,
        agents_running,
        ...old
      } = fake.status
      void [user_only_minutes, agent_only_minutes, both_minutes, agents_running]

      if (
        !isUnknown &&
        argv[0] === 'hook' &&
        argv[1] === 'prompt' &&
        fake.nudge !== null
      ) {
        stdout = JSON.stringify({
          hookSpecificOutput: {
            hookEventName: 'UserPromptSubmit',
            additionalContext: `Relay this to the user. Message to relay: "${fake.nudge.message}"`,
          },
        })
        fake.nudge = null
      } else if (argv[0] === 'status') {
        stdout = JSON.stringify(old)
      }

      return {
        value: {
          exitCode: isUnknown || argv[1] === 'tick' ? 2 : 0,
          stdout,
          stderr: isUnknown ? "error: unexpected argument '--json' found" : '',
          isStdoutTruncated: false,
          isStderrTruncated: false,
        },
      }
    }

    if (argv[0] === 'hook') {
      const event = argv[1]
      const nudge = event === 'prompt' || event === 'tick' ? fake.nudge : null
      fake.nudge = nudge === null ? fake.nudge : null
      stdout = JSON.stringify({
        event,
        version: '0.4.0',
        nudge,
        status: fake.status,
        ...(event === 'session-start'
          ? { classic_hooks: fake.hasSettingsHooks }
          : {}),
      })
    } else if (argv[0] === 'ignore') {
      fake.status = { ...fake.status, ignored: !fake.status.ignored }
      stdout = fake.status.ignored
        ? 'garlic: nudging disabled for today (tracking continues)\n'
        : 'garlic: nudging resumed\n'
    } else if (argv[0] === 'reset') {
      fake.status = {
        ...fake.status,
        accumulated_minutes: 0,
        agent_minutes: 0,
        user_minutes: 0,
        user_only_minutes: 0,
        agent_only_minutes: 0,
        both_minutes: 0,
        overlap_minutes: 0,
        nudges_given: [],
        next_threshold: 60,
      }
    } else if (argv[0] === 'status') {
      stdout = 'Mon  1h 10m\nTue  2h 15m\n'
    }

    return {
      value: {
        exitCode: 0,
        stdout,
        stderr: '',
        isStdoutTruncated: false,
        isStderrTruncated: false,
      },
    }
  })

  return fake
}

/** `/garlic <args>` as the person types it at the prompt. */
const typed = (args = '') =>
  ({
    command: 'garlic',
    args,
    origin: { kind: 'composer' },
    presentation: { isFullscreen: true, columns: 120 },
  }) as unknown as CommandRunInput

const START = { cwd: '/', surface: 'terminal', isInteractive: true } as const
const hooks = (fake: Fake) =>
  fake.calls.filter(argv => argv[0] === 'hook').map(argv => argv[1])

test('a turn is handed to the binary, an interrupt and all', async ($, on) => {
  const fake = world(on)
  mock.clock(on)

  await $.session.start(START)
  await $.turn.start({ text: 'fix the bug', turnId: 't1' })
  // A subagent's turn inside the main one is not an event of the person's.
  await $.turn.start({ text: 'explore', turnId: 't2', agentId: 'a1' })
  await $.turn.complete({
    answer: '',
    durationMs: 10,
    isAborted: false,
    turnId: 't2',
    agentId: 'a1',
    reason: 'answer',
  })
  // The person pressed Esc: still the end of the agent's interval.
  await $.turn.complete({
    answer: '',
    durationMs: 10,
    isAborted: true,
    turnId: 't1',
    reason: 'aborted',
  })

  expect(hooks(fake)).toEqual(['session-start', 'prompt', 'stop'])
  expect(fake.calls[1]).toEqual(['hook', 'prompt', '--json'])
  expect(fake.stdin[1]).toBe('{"session_id":"session-1"}')
  expect(fake.statusLines.at(-1)).toBe(
    '\u{1F9DB} 2h 15m / 4h 00m · you 1h 00m · agents 1h 30m',
  )
})

test('the readout moves between events, and ends with the session', async ($, on) => {
  const fake = world(on)
  const clock = mock.clock(on)

  await $.session.start(START)
  await clock.advance(60_000)
  await clock.advance(60_000)
  expect(hooks(fake)).toEqual(['session-start', 'tick', 'tick'])

  await $.session.end({
    reason: 'prompt_input_exit',
    sessionId: 'session-1',
    resume: { id: 'session-1' },
  })
  await clock.advance(180_000)
  expect(hooks(fake)).toEqual(['session-start', 'tick', 'tick', 'session-end'])
})

test('a nudge is shown to the person, with its buttons', async ($, on) => {
  const fake = world(on)
  mock.clock(on)
  fake.nudge = {
    message: "You've been at it for 2h. Stretch your legs.",
    kind: 'threshold',
    threshold_minutes: 120,
  }

  await $.session.start(START)
  await $.turn.start({ text: 'next', turnId: 't1' })
  expect(fake.toasts).toEqual([
    "\u{1F9C4} You've been at it for 2h. Stretch your legs.",
  ])

  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...BAND, surface })
    expect(
      await ui.find({ type: 'Text', text: /Stretch your legs/ }),
    ).toBeDefined()
    expect(await ui.find({ key: 'nudge-dismiss' })).toBeDefined()
    await ui.unmount()
  }

  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await ui.press({ key: 'nudge-pause' })
  expect(fake.calls.some(argv => argv[0] === 'ignore')).toBe(true)
  expect(fake.status.ignored).toBe(true)
  expect(fake.statusLines.at(-1)).toMatch(/nudges paused/)
  // Pausing answers the nudge: the band is the engine's again.
  expect(await ui.find({ key: 'nudge-pause' })).toBeUndefined()
  await ui.unmount()
})

test('the panel shows you against your agents on every surface', async ($, on) => {
  const fake = world(on)
  mock.clock(on)
  await $.session.start(START)
  await $.command.run(typed())
  expect(hooks(fake)).toEqual(['session-start', 'tick'])

  for (const surface of ['terminal', 'desktop', 'vscode', 'mobile'] as const) {
    const ui = await $.ui.mount({ ...PANE, surface })
    expect(
      (await ui.find({ type: 'Text', text: /in Claude Code today/ }))?.text,
    ).toMatch(/2h 15m in Claude Code today of 4h 00m/)
    expect(
      (await ui.find({ type: 'Text', text: /only agents/ }))?.text,
    ).toMatch(/only you 45m\s+. both 15m\s+. only agents 1h 15m/)
    expect((await ui.find({ type: 'Text', text: /Next nudge/ }))?.text).toBe(
      'Next nudge at 3h 00m, 45m to go.',
    )
    expect(
      (await ui.find({ type: 'Text', text: /sessions at once/ }))?.text,
    ).toMatch(
      /you 1h 00m · agents 1h 30m · 12m with 2\+ sessions at once · 1 agent running now/,
    )
    await ui.unmount()
  }
})

test('resetting the day asks first', async ($, on) => {
  const fake = world(on)
  mock.clock(on)
  await $.session.start(START)

  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await ui.press({ key: 'reset' })
  expect(fake.calls.some(argv => argv[0] === 'reset')).toBe(false)
  expect(
    await ui.find({ type: 'Text', text: /Reset today to zero/ }),
  ).toBeDefined()

  await ui.press({ key: 'reset-no' })
  expect(fake.calls.some(argv => argv[0] === 'reset')).toBe(false)

  await ui.press({ key: 'reset' })
  await ui.press({ key: 'reset-yes' })
  expect(fake.calls).toContainEqual(['reset', '-y'])
  expect(
    (await ui.find({ type: 'Text', text: /in Claude Code today/ }))?.text,
  ).toMatch(/0m in Claude Code today/)
  await ui.unmount()
})

test('the command answers pause, week and nonsense without the panel', async ($, on) => {
  const fake = world(on)
  mock.clock(on)
  await $.session.start(START)

  expect((await $.command.run(typed('pause'))).text).toBe(
    'garlic: nudging disabled for today (tracking continues)',
  )
  expect((await $.command.run(typed('week'))).text).toBe(
    'Mon  1h 10m\nTue  2h 15m',
  )
  expect(fake.calls).toContainEqual(['status', '--week'])
  expect((await $.command.run(typed('delete everything'))).text).toMatch(
    /try \/garlic/,
  )
  // `/garlic reset` never resets by itself: it opens the question.
  await $.command.run(typed('reset'))
  expect(fake.calls.some(argv => argv[0] === 'reset')).toBe(false)
  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect(await ui.find({ key: 'reset-yes' })).toBeDefined()
  await ui.unmount()
})

test('leftover settings hooks are offered for removal', async ($, on) => {
  const fake = world(on)
  mock.clock(on)
  fake.hasSettingsHooks = true
  await $.session.start(START)

  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await ui.press({ key: 'hooks-remove' })
  expect(fake.calls).toContainEqual(['setup', '--remove-hooks'])
  expect(await ui.find({ key: 'hooks-remove' })).toBeUndefined()
  await ui.unmount()
})

test('without the binary it says so once and stands down', async ($, on) => {
  const fake = world(on, false)
  const clock = mock.clock(on)

  await $.session.start(START)
  await $.turn.start({ text: 'hello', turnId: 't1' })
  await clock.advance(180_000)

  expect(fake.toasts.length).toBe(1)
  expect(fake.toasts[0]).toMatch(/garlic is not installed.*brew install --cask/)
  expect((await $.command.run(typed())).text).toMatch(/garlic is not installed/)
  // Nothing of the engine's is taken over.
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Button' })).toBeUndefined()
  await ui.unmount()
})

test('a garlic from before --json keeps tracking', async ($, on) => {
  const fake = world(on, true, true)
  mock.clock(on)
  fake.nudge = {
    message: 'Stretch your legs.',
    kind: 'threshold',
    threshold_minutes: 120,
  }

  await $.session.start(START)
  await $.turn.start({ text: 'hi', turnId: 't1' })
  await $.turn.complete({
    answer: 'done',
    durationMs: 10,
    isAborted: false,
    turnId: 't1',
    reason: 'answer',
  })

  const plain = fake.calls.filter(
    argv => argv[0] === 'hook' && !argv.includes('--json'),
  )
  expect(plain.map(argv => argv[1])).toEqual([
    'session-start',
    'prompt',
    'stop',
  ])
  expect(fake.statusLines.at(-1)).toContain('2h 15m')
  expect(fake.toasts.some(text => text.includes('0.4.0'))).toBe(true)
  expect(fake.toasts.some(text => text.includes('Stretch your legs.'))).toBe(
    true,
  )
  expect(fake.toasts.some(text => text.includes('Relay this'))).toBe(false)
})

test('the helpers read and format what the binary prints', () => {
  expect(duration(0)).toBe('0m')
  expect(duration(59.9)).toBe('59m')
  expect(duration(65)).toBe('1h 05m')
  expect(bar(0.5, 10)).toBe('█████░░░░░')
  expect(bar(7, 4)).toBe('████')

  const status = toStatus(STATUS, 0)
  expect(status).not.toBeNull()

  if (status !== null) {
    const split = shares(status, 40)
    expect(split.youOnly + split.both + split.agentsOnly).toBe(40)
    expect(split.agentsOnly).toBeGreaterThan(split.youOnly)
    // A sliver still gets a cell, and the cells still add up.
    const sliver = shares(
      { ...status, youOnly: 0.2, both: 0, agentsOnly: 300 },
      20,
    )
    expect(sliver).toEqual({ youOnly: 1, both: 0, agentsOnly: 19 })
    expect(statusLine({ ...status, isPaused: true, agentsRunning: 3 })).toBe(
      '\u{1F9DB} 2h 15m / 4h 00m · you 1h 00m · agents 1h 30m · 3 running · nudges paused',
    )
  }

  expect(toStatus({ nope: true }, 0)).toBeNull()
  expect(parseReply('not json', 0)).toBeNull()
  expect(parseReply('{"status":null,"nudge":null}', 0)).toEqual({
    status: null,
    nudge: null,
  })
  expect(isAtLeast('0.4.0', '0.4.0')).toBe(true)
  expect(isAtLeast('0.10.1', '0.4.0')).toBe(true)
  expect(isAtLeast('0.3.5', '0.4.0')).toBe(false)
  expect(parseAsk('')).toEqual({ kind: 'panel' })
  expect(parseAsk('status --month').kind).toBe('report')
  expect(parseAsk('ignore')).toEqual({ kind: 'pause' })
  expect(parseAsk('rm -rf')).toEqual({ kind: 'unknown' })
})
