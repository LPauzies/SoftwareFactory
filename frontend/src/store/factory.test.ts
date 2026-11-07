import test from 'node:test'
import assert from 'node:assert/strict'
import {
  useAgentActivityState,
  evaluateTicketActivity,
  type AgentStepExecution,
  type FactoryTicket,
  type PipelineRun,
} from './factory'

function makeStep(overrides: Partial<AgentStepExecution> = {}): AgentStepExecution {
  return {
    role: 'DEV',
    title: 'Software Craftsman (DEV)',
    wagner_pillar: 'Code & TDD',
    model: 'gemini-4-argon-medium',
    status: 'pending',
    started_at: null,
    completed_at: null,
    duration_seconds: 0,
    agy_conversation_id: null,
    agy_command: 'agy --mode=auto',
    summary: 'Queued in WAGNER Factory...',
    markdown_output: '',
    thinking_log: [],
    artifacts: [],
    usage: {
      input_tokens: 0,
      output_tokens: 0,
      thinking_tokens: 0,
      cache_read_tokens: 0,
      total_tokens: 0,
    },
    quality_gate_passed: true,
    ...overrides,
  }
}

function makeTicket(overrides: Partial<FactoryTicket> = {}): FactoryTicket {
  return {
    id: 'tk-24',
    identifier: 'CAI-24',
    title: 'Animation cartes et panel droit',
    description: 'Visualiser si un agent tourne ou a planté',
    priority_label: 'High',
    linear_project_id: 'proj-1',
    linear_url: null,
    repository_path: '/home/mediaserver/Documents/SoftwareFactory',
    stage: 'coding',
    active_role: null,
    active_run_id: 'run-24',
    version_tag: 'v1.9.0',
    updated_at: '2026-11-07T15:00:00.000Z',
    ...overrides,
  }
}

function makeRun(
  steps: AgentStepExecution[],
  overrides: Partial<PipelineRun> = {}
): PipelineRun {
  return {
    id: 'run-24',
    project_id: 'proj-1',
    ticket_id: 'tk-24',
    ticket_identifier: 'CAI-24',
    version_tag: 'v1.9.0',
    feature_request: 'CAI-24',
    execution_mode: 'agy_live',
    status: 'running',
    created_at: '2026-11-07T15:00:00.000Z',
    completed_at: null,
    steps,
    test_report: null,
    living_docs: null,
    total_tokens: 0,
    total_duration_seconds: 0,
    workspace_dir: '/home/mediaserver/Documents/SoftwareFactory',
    ...overrides,
  }
}

test('useAgentActivityState reports running with live elapsed seconds when agent is active', () => {
  const nowMs = Date.parse('2026-11-07T15:00:15.000Z')
  const step = makeStep({
    status: 'running',
    started_at: '2026-11-07T15:00:00.000Z',
    thinking_log: [
      {
        id: 't-1',
        role: 'DEV',
        kind: 'reasoning',
        content: 'Inspecting Vue components',
        timestamp: '2026-11-07T15:00:10.000Z',
      },
    ],
  })
  const ticket = makeTicket({ active_role: 'DEV' })
  const run = makeRun([step])

  const info = useAgentActivityState(step, ticket, run, { nowMs })
  assert.equal(info.state, 'running')
  assert.equal(info.isRunning, true)
  assert.equal(info.isCrashed, false)
  assert.equal(info.elapsedSeconds, 15)
  assert.equal(info.lastSignalAgeSeconds, 5)

  const tkInfo = evaluateTicketActivity(ticket, run, { nowMs })
  assert.equal(tkInfo.state, 'running')
  assert.equal(tkInfo.isRunning, true)
  assert.equal(tkInfo.activeRole, 'DEV')
})

test('useAgentActivityState transitions immediately to crashed on step failure or quality gate rejection', () => {
  const step = makeStep({
    status: 'failed',
    quality_gate_passed: false,
    summary: 'Quality Gate Rejected: No real source code modified',
    thinking_log: [
      {
        id: 't-err',
        role: 'DEV',
        kind: 'error',
        content: 'FATAL DEV FAILURE: Subprocess failed or timed out',
        timestamp: '2026-11-07T15:01:00.000Z',
      },
    ],
  })
  const ticket = makeTicket({ active_role: null, stage: 'coding' })
  const run = makeRun([step], { status: 'failed' })

  const info = useAgentActivityState(step, ticket, run)
  assert.equal(info.state, 'crashed')
  assert.equal(info.isRunning, false)
  assert.equal(info.isCrashed, true)
  assert.match(info.errorMessage || '', /FATAL DEV FAILURE/)

  const tkInfo = evaluateTicketActivity(ticket, run)
  assert.equal(tkInfo.state, 'crashed')
  assert.equal(tkInfo.isCrashed, true)
  assert.equal(tkInfo.failedRole, 'DEV')
})

test('useAgentActivityState detects heartbeat expiration when enforceHeartbeatTimeout is enabled', () => {
  const nowMs = Date.parse('2026-11-07T15:01:00.000Z')
  const step = makeStep({
    status: 'running',
    started_at: '2026-11-07T15:00:00.000Z',
    thinking_log: [
      {
        id: 't-1',
        role: 'DEV',
        kind: 'reasoning',
        content: 'Initial thought',
        timestamp: '2026-11-07T15:00:10.000Z',
      },
    ],
  })
  const ticket = makeTicket({ active_role: 'DEV' })
  const run = makeRun([step])

  const info = useAgentActivityState(step, ticket, run, {
    nowMs,
    heartbeatTimeoutMs: 30_000,
    enforceHeartbeatTimeout: true,
  })
  assert.equal(info.state, 'crashed')
  assert.equal(info.isCrashed, true)
  assert.match(info.errorMessage || '', /Heartbeat timeout/)
})

test('useAgentActivityState reports waiting when agent is paused on operator question', () => {
  const step = makeStep({
    role: 'ARCHI',
    status: 'running',
    started_at: '2026-11-07T15:00:00.000Z',
  })
  const ticket = makeTicket({
    active_role: 'ARCHI',
    pending_question: 'Static or dynamic heartbeat threshold?',
  })
  const run = makeRun([step])

  const info = useAgentActivityState(step, ticket, run)
  assert.equal(info.state, 'waiting')
  assert.equal(info.isRunning, false)
  assert.equal(info.isCrashed, false)
})
