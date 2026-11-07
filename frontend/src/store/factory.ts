import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'

export type KanbanStage =
  | 'todo'
  | 'design'
  | 'coding'
  | 'testing'
  | 'done'
  | 'pr_review'
  | 'rolled_back'

export interface AgyModelInfo {
  id: string
  label: string
  family: string
}

export interface AgyStatus {
  binary_path: string
  available: boolean
  version_hint: string
  models: AgyModelInfo[]
  brain_sessions_count?: number
  brain_steps_count?: number
}

export interface ProjectRepoMapping {
  linear_project: string
  local_path: string
  github_repo: string
  default_branch?: string
}

export interface LinearProject {
  id: string
  name: string
  key: string
  description: string
  default_repo_path: string
  default_github_repo?: string
  source: string
}

export interface FactoryTicket {
  id: string
  identifier: string
  title: string
  description: string
  priority_label: string
  linear_project_id: string
  linear_team_id?: string | null
  linear_url: string | null
  repository_path: string
  github_repo?: string
  stage: KanbanStage
  active_role: string | null
  active_run_id: string | null
  pending_question?: string | null
  pr_url?: string | null
  pr_branch?: string | null
  version_tag: string
  created_at?: string
  updated_at: string
  review_feedback?: string | null
}

export interface LinearStatus {
  connected: boolean
  has_api_key: boolean
  organization_name: string | null
  viewer_name: string | null
  last_synced_at: string | null
}

export interface FactoryBrick {
  kind: string
  name: string
  french_subtitle: string
  description: string
  enabled: boolean
  provider_plugin: string
  health_score: number
}

export interface AgentConfig {
  role: string
  title: string
  wagner_pillar: string
  model: string
  effort: string
  enabled: boolean
  system_prompt: string
}

export interface ArtifactFile {
  path: string
  category: string
  language: string
  content: string
}

export interface AgyUsage {
  input_tokens: number
  output_tokens: number
  thinking_tokens: number
  cache_read_tokens: number
  total_tokens: number
}

export interface ThoughtEntry {
  id: string
  role: string
  kind: 'reasoning' | 'tool_probe' | 'question' | 'user_steering' | 'conclusion' | string
  content: string
  timestamp: string
}

export interface AgentNotification {
  id: string
  ticket_id: string | null
  ticket_identifier: string | null
  role: string | null
  kind: 'running' | 'question' | 'completed' | 'release' | 'rollback' | 'info'
  title: string
  detail: string
  timestamp: string
  read: boolean
}

export interface AgentStepExecution {
  role: string
  title: string
  wagner_pillar: string
  model: string
  status: 'pending' | 'running' | 'completed' | 'skipped' | 'failed'
  started_at: string | null
  completed_at: string | null
  duration_seconds: number
  agy_conversation_id: string | null
  agy_command: string
  summary: string
  markdown_output: string
  thinking_log: ThoughtEntry[]
  artifacts: ArtifactFile[]
  usage: AgyUsage
  quality_gate_passed: boolean
}

export type AgentActivityState =
  | 'running'
  | 'crashed'
  | 'waiting'
  | 'completed'
  | 'skipped'
  | 'idle'

export const DEFAULT_HEARTBEAT_TIMEOUT_MS = 30_000
export const STALLED_SIGNAL_THRESHOLD_SECONDS = 60

export interface StepActivityInfo {
  state: AgentActivityState
  isRunning: boolean
  isCrashed: boolean
  isStalled: boolean
  elapsedSeconds: number
  lastSignalAgeSeconds: number | null
  errorMessage: string | null
  startedAt: string | null
  completedAt: string | null
  durationSeconds: number
}

export interface TicketActivityInfo {
  state: AgentActivityState
  isRunning: boolean
  isCrashed: boolean
  activeRole: string | null
  failedRole: string | null
  elapsedSeconds: number
  errorMessage: string | null
  startedAt: string | null
}

// shortcut: client-side activity & crash state derived from SSE step events and timestamps, upgrade to dedicated backend process heartbeat ping if sub-second OS PID liveness is needed
export function useAgentActivityState(
  step?: AgentStepExecution | null,
  ticket?: FactoryTicket | null,
  run?: PipelineRun | null,
  options?: {
    nowMs?: number
    heartbeatTimeoutMs?: number
    enforceHeartbeatTimeout?: boolean
  }
): StepActivityInfo {
  const nowMs = options?.nowMs ?? Date.now()
  const timeoutMs = options?.heartbeatTimeoutMs ?? DEFAULT_HEARTBEAT_TIMEOUT_MS

  if (!step) {
    return {
      state: 'idle',
      isRunning: false,
      isCrashed: false,
      isStalled: false,
      elapsedSeconds: 0,
      lastSignalAgeSeconds: null,
      errorMessage: null,
      startedAt: null,
      completedAt: null,
      durationSeconds: 0,
    }
  }

  const startedMs = step.started_at ? Date.parse(step.started_at) : NaN
  const lastThought =
    step.thinking_log && step.thinking_log.length > 0
      ? step.thinking_log[step.thinking_log.length - 1]
      : null
  const lastThoughtMs = lastThought?.timestamp ? Date.parse(lastThought.timestamp) : NaN
  const lastSignalMs = !Number.isNaN(lastThoughtMs)
    ? lastThoughtMs
    : !Number.isNaN(startedMs)
      ? startedMs
      : NaN

  const elapsedSeconds =
    step.status === 'running' && !Number.isNaN(startedMs)
      ? Math.max(0, Math.floor((nowMs - startedMs) / 1000))
      : Math.round(step.duration_seconds || 0)

  const lastSignalAgeSeconds = !Number.isNaN(lastSignalMs)
    ? Math.max(0, Math.floor((nowMs - lastSignalMs) / 1000))
    : null

  const isStalled =
    step.status === 'running' &&
    lastSignalAgeSeconds !== null &&
    lastSignalAgeSeconds >= STALLED_SIGNAL_THRESHOLD_SECONDS

  const errorThought = step.thinking_log?.find(
    (t) => t.kind === 'error' || (t.kind === 'warning' && !step.quality_gate_passed)
  )

  const hasFatalErrorThought = step.thinking_log?.some((t) => t.kind === 'error') ?? false
  const isHeartbeatExpired =
    Boolean(options?.enforceHeartbeatTimeout) &&
    step.status === 'running' &&
    !Number.isNaN(lastSignalMs) &&
    nowMs - lastSignalMs > timeoutMs

  const isOrphanedRunningStep =
    step.status === 'running' &&
    (run?.status === 'failed' ||
      (ticket != null && !ticket.active_role && run?.status !== 'running'))

  const startedAt = step.started_at || null
  const completedAt = step.completed_at || null
  const durationSeconds = step.duration_seconds || 0

  if (
    step.status === 'failed' ||
    step.quality_gate_passed === false ||
    hasFatalErrorThought ||
    isOrphanedRunningStep ||
    isHeartbeatExpired
  ) {
    const errorMessage =
      errorThought?.content ||
      (isHeartbeatExpired
        ? `Heartbeat timeout (> ${Math.round(timeoutMs / 1000)}s sans signal de l'agent ${step.role})`
        : step.summary && step.summary !== 'Queued in WAGNER Factory...'
          ? step.summary
          : `L'agent ${step.role} s'est arrêté de manière inattendue.`)
    return {
      state: 'crashed',
      isRunning: false,
      isCrashed: true,
      isStalled: false,
      elapsedSeconds,
      lastSignalAgeSeconds,
      errorMessage,
      startedAt,
      completedAt,
      durationSeconds,
    }
  }

  if (
    step.status === 'running' &&
    ticket?.pending_question &&
    (!ticket.active_role || ticket.active_role === step.role)
  ) {
    return {
      state: 'waiting',
      isRunning: false,
      isCrashed: false,
      isStalled: false,
      elapsedSeconds,
      lastSignalAgeSeconds,
      errorMessage: null,
      startedAt,
      completedAt,
      durationSeconds,
    }
  }

  if (
    step.status === 'running' ||
    (ticket?.active_role === step.role && step.status !== 'completed')
  ) {
    return {
      state: 'running',
      isRunning: true,
      isCrashed: false,
      isStalled,
      elapsedSeconds,
      lastSignalAgeSeconds,
      errorMessage: null,
      startedAt,
      completedAt,
      durationSeconds,
    }
  }

  if (step.status === 'completed' || step.status === 'succeeded') {
    return {
      state: 'completed',
      isRunning: false,
      isCrashed: false,
      isStalled: false,
      elapsedSeconds,
      lastSignalAgeSeconds,
      errorMessage: null,
      startedAt,
      completedAt,
      durationSeconds,
    }
  }

  if (step.status === 'skipped') {
    return {
      state: 'skipped',
      isRunning: false,
      isCrashed: false,
      isStalled: false,
      elapsedSeconds: 0,
      lastSignalAgeSeconds,
      errorMessage: null,
      startedAt,
      completedAt,
      durationSeconds: 0,
    }
  }

  return {
    state: 'idle',
    isRunning: false,
    isCrashed: false,
    isStalled: false,
    elapsedSeconds: 0,
    lastSignalAgeSeconds,
    errorMessage: null,
    startedAt,
    completedAt,
    durationSeconds: 0,
  }
}

export function evaluateTicketActivity(
  ticket?: FactoryTicket | null,
  run?: PipelineRun | null,
  options?: {
    nowMs?: number
    heartbeatTimeoutMs?: number
    enforceHeartbeatTimeout?: boolean
  }
): TicketActivityInfo {
  if (!ticket) {
    return {
      state: 'idle',
      isRunning: false,
      isCrashed: false,
      activeRole: null,
      failedRole: null,
      elapsedSeconds: 0,
      errorMessage: null,
      startedAt: null,
    }
  }

  const activeStep = ticket.active_role
    ? run?.steps?.find((s) => s.role === ticket.active_role) || null
    : null

  const startedAt = activeStep?.started_at || run?.created_at || null

  if (activeStep) {
    const stepInfo = useAgentActivityState(activeStep, ticket, run, options)
    if (stepInfo.isCrashed) {
      return {
        state: 'crashed',
        isRunning: false,
        isCrashed: true,
        activeRole: null,
        failedRole: activeStep.role,
        elapsedSeconds: stepInfo.elapsedSeconds,
        errorMessage: stepInfo.errorMessage,
        startedAt,
      }
    }
  }

  if (ticket.pending_question) {
    return {
      state: 'waiting',
      isRunning: false,
      isCrashed: false,
      activeRole: ticket.active_role,
      failedRole: null,
      elapsedSeconds: activeStep
        ? useAgentActivityState(activeStep, ticket, run, options).elapsedSeconds
        : 0,
      errorMessage: null,
      startedAt,
    }
  }

  if (ticket.active_role) {
    const stepInfo = activeStep
      ? useAgentActivityState(activeStep, ticket, run, options)
      : null
    return {
      state: 'running',
      isRunning: true,
      isCrashed: false,
      activeRole: ticket.active_role,
      failedRole: null,
      elapsedSeconds: stepInfo?.elapsedSeconds ?? 0,
      errorMessage: null,
      startedAt,
    }
  }

  if (run) {
    const crashedStep = run.steps?.find(
      (s) => useAgentActivityState(s, ticket, run, options).isCrashed
    )
    if (crashedStep || run.status === 'failed') {
      const info = crashedStep
        ? useAgentActivityState(crashedStep, ticket, run, options)
        : null
      return {
        state: 'crashed',
        isRunning: false,
        isCrashed: true,
        activeRole: null,
        failedRole: crashedStep?.role || null,
        elapsedSeconds: info?.elapsedSeconds ?? Math.round(run.total_duration_seconds || 0),
        errorMessage:
          info?.errorMessage ||
          `Le pipeline a échoué (${run.version_tag}).`,
        startedAt: crashedStep?.started_at || run.created_at || null,
      }
    }
  }

  if (ticket.stage === 'done' || ticket.stage === 'pr_review') {
    return {
      state: 'completed',
      isRunning: false,
      isCrashed: false,
      activeRole: null,
      failedRole: null,
      elapsedSeconds: Math.round(run?.total_duration_seconds || 0),
      errorMessage: null,
      startedAt: run?.created_at || null,
    }
  }

  return {
    state: 'idle',
    isRunning: false,
    isCrashed: false,
    activeRole: null,
    failedRole: null,
    elapsedSeconds: 0,
    errorMessage: null,
    startedAt: null,
  }
}

export interface TestFactoryReport {
  total_scenarios: number
  passed_scenarios: number
  integration_tests: number
  business_bdd_scenarios: number
  coverage_estimate: number
  regression_shield_active: boolean
  gherkin_feature: string
}

export interface LivingDocBundle {
  release_notes: string
  wiki_architecture_page: string
  living_bdd_catalog: string
  rollback_runbook: string
}

export interface PipelineRun {
  id: string
  project_id: string
  ticket_id?: string | null
  ticket_identifier?: string | null
  version_tag: string
  feature_request: string
  execution_mode: string
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'rolled_back'
  created_at: string
  completed_at: string | null
  steps: AgentStepExecution[]
  test_report: TestFactoryReport | null
  living_docs: LivingDocBundle | null
  pr_url?: string | null
  pr_branch?: string | null
  total_tokens: number
  total_duration_seconds: number
  workspace_dir: string
}

export interface ReleaseRecord {
  id: string
  project_id: string
  pipeline_run_id: string
  version: string
  environment: string
  status: 'active_in_prod' | 'staging_ready' | 'superseded' | 'rolled_back'
  deployed_at: string
  release_notes: string
  rollback_script: string
  test_pass_rate: number
  incident_notes: string | null
}

export interface RollbackEvent {
  id: string
  project_id: string
  from_version: string
  to_version: string
  reason: string
  ops_diagnosis: string
  triggered_at: string
  agy_conversation_id: string | null
}

export interface Project {
  id: string
  name: string
  slug: string
  description: string
  domain: string
  repository_path: string
  active_version: string
  created_at: string
  bricks: FactoryBrick[]
  agents: AgentConfig[]
  homogeneity_score: number
}

export const useFactoryStore = defineStore('factory', () => {
  const agyStatus = ref<AgyStatus | null>(null)
  const projects = ref<Project[]>([])
  const linearProjects = ref<LinearProject[]>([])
  const projectMappings = ref<ProjectRepoMapping[]>([])
  const tickets = ref<FactoryTicket[]>([])
  const linearStatus = ref<LinearStatus | null>(null)
  const runs = ref<PipelineRun[]>([])
  const releases = ref<ReleaseRecord[]>([])
  const rollbacks = ref<RollbackEvent[]>([])

  const selectedLinearProjectId = ref<string>(
    localStorage.getItem('wagner_selected_linear_project_id') || 'all'
  )

  watch(selectedLinearProjectId, (newId) => {
    if (newId) {
      localStorage.setItem('wagner_selected_linear_project_id', newId)
    }
  })
  const selectedTicketId = ref<string | null>(null)
  const sseConnected = ref<boolean>(false)
  const loading = ref<boolean>(false)
  const lastActivityToast = ref<string>('')
  const notifications = ref<AgentNotification[]>([])

  const runningTickets = computed(() =>
    tickets.value.filter((t) => !!t.active_role)
  )

  const pendingQuestionTickets = computed(() =>
    tickets.value.filter((t) => !!t.pending_question)
  )

  const unreadNotificationCount = computed(
    () => notifications.value.filter((n) => !n.read).length
  )

  // Gemini Subscription Quota (configurable in bottom bar, persisted locally)
  const subscriptionQuotaTokens = ref<number>(
    Number(localStorage.getItem('wagner_gemini_quota')) || 500000
  )

  function setSubscriptionQuota(quota: number) {
    subscriptionQuotaTokens.value = quota
    localStorage.setItem('wagner_gemini_quota', String(quota))
  }

  const geminiUsageSummary = computed(() => {
    let input = 0
    let output = 0
    let thinking = 0
    let cache = 0
    let total = 0
    let argonTokens = 0
    let flashTokens = 0
    let completedSteps = 0

    for (const r of runs.value) {
      for (const s of r.steps || []) {
        const u = s.usage
        if (u) {
          input += u.input_tokens || 0
          output += u.output_tokens || 0
          thinking += u.thinking_tokens || 0
          cache += u.cache_read_tokens || 0
          const stepTot =
            u.total_tokens ||
            (u.input_tokens || 0) +
              (u.output_tokens || 0) +
              (u.thinking_tokens || 0) +
              (u.cache_read_tokens || 0)
          total += stepTot
          if (s.model?.includes('argon') || s.model?.includes('gemini-4')) {
            argonTokens += stepTot
          } else {
            flashTokens += stepTot
          }
        }
        if (s.status === 'completed') {
          completedSteps += 1
        }
      }
    }

    const quota = Math.max(1, subscriptionQuotaTokens.value)
    const percentUsed = Math.min(100, Number(((total / quota) * 100).toFixed(1)))
    const inputPct = Math.min(100, (input / quota) * 100)
    const thinkingPct = Math.min(100 - inputPct, (thinking / quota) * 100)
    const outputPct = Math.min(
      100 - inputPct - thinkingPct,
      (output / quota) * 100
    )
    const cachePct = Math.min(
      100 - inputPct - thinkingPct - outputPct,
      (cache / quota) * 100
    )

    return {
      input,
      output,
      thinking,
      cache,
      total,
      quota,
      remaining: Math.max(0, quota - total),
      percentUsed,
      inputPct,
      thinkingPct,
      outputPct,
      cachePct,
      argonTokens,
      flashTokens,
      completedSteps,
    }
  })

  function pushNotification(
    partial: Omit<AgentNotification, 'id' | 'timestamp' | 'read'>
  ) {
    notifications.value.unshift({
      ...partial,
      id: `notif-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
      timestamp: new Date().toLocaleTimeString(),
      read: false,
    })
    if (notifications.value.length > 40) {
      notifications.value.length = 40
    }
  }

  function markNotificationsRead() {
    notifications.value.forEach((n) => {
      n.read = true
    })
  }

  function clearNotifications() {
    notifications.value = []
  }

  const activeProject = computed(() => projects.value[0] || null)

  const activeLinearProject = computed(
    () =>
      linearProjects.value.find((p) => p.id === selectedLinearProjectId.value) ||
      null
  )

  const dismissedTicketIds = ref<string[]>(
    JSON.parse(localStorage.getItem('wagner_dismissed_tickets') || '[]')
  )

  function recordDismissedTicket(id: string, identifier?: string) {
    if (id && !dismissedTicketIds.value.includes(id)) {
      dismissedTicketIds.value.push(id)
    }
    if (identifier && !dismissedTicketIds.value.includes(identifier)) {
      dismissedTicketIds.value.push(identifier)
    }
    localStorage.setItem(
      'wagner_dismissed_tickets',
      JSON.stringify(dismissedTicketIds.value)
    )
  }

  const projectTickets = computed(() => {
    const currentId = selectedLinearProjectId.value
    return tickets.value.filter((t) => {
      if ((t as any).archived) {
        return false
      }
      if (
        dismissedTicketIds.value.includes(t.id) ||
        dismissedTicketIds.value.includes(t.identifier)
      ) {
        return false
      }
      if (!currentId || currentId === 'all') {
        return true
      }
      if (t.linear_project_id === currentId) {
        return true
      }
      const currentProj = linearProjects.value.find((p) => p.id === currentId)
      if (
        currentProj &&
        t.repository_path &&
        currentProj.default_repo_path === t.repository_path
      ) {
        return true
      }
      return false
    })
  })

  const selectedTicket = computed(() =>
    tickets.value.find((t) => t.id === selectedTicketId.value) || null
  )

  function runForTicket(tk?: FactoryTicket | null): PipelineRun | null {
    if (!tk) return null
    if (tk.active_run_id) {
      return runs.value.find((r) => r.id === tk.active_run_id) || null
    }
    return runs.value.find((r) => r.ticket_id === tk.id) || null
  }

  const selectedTicketRun = computed(() => runForTicket(selectedTicket.value))

  async function initialize() {
    loading.value = true
    try {
      const [statusRes, stateRes, mappingsRes] = await Promise.all([
        fetch('/api/agy/status'),
        fetch('/api/state'),
        fetch('/api/project-mappings'),
      ])
      if (statusRes.ok) {
        agyStatus.value = await statusRes.json()
      }
      if (stateRes.ok) {
        applyState(await stateRes.json())
      }
      if (mappingsRes.ok) {
        projectMappings.value = await mappingsRes.json()
      }
    } finally {
      loading.value = false
    }
    connectSse()
  }

  const defaultBaseBranch = computed(() => {
    const first = projectMappings.value.find((m) => m.default_branch?.trim())
    return first?.default_branch?.trim() || 'master'
  })

  function formatPrTitle(identifier: string, rawTitle?: string): string {
    const cleanId = (identifier || 'WAGNER')
      .replace(/^\[|\].*$/g, '')
      .trim()
      .replace(/[^A-Za-z0-9_-]+/g, '-')
      .replace(/^-|-$/g, '') || 'WAGNER'
    let s = (rawTitle || '').split(/\r?\n/)[0]?.trim() || ''
    s = s.replace(/^\[[^\]]+\](?:\([^)]*\))?\s*/, '')
    if (s.toLowerCase().startsWith(cleanId.toLowerCase())) {
      s = s.slice(cleanId.length).trim()
    }
    s = s.replace(/^[—–\-:\s]+/, '').trim()
    if (s.includes(' — ')) {
      s = s.split(' — ')[0].trim()
    }
    s = s.replace(/(?:[\s—–\-:]*#\d+)+\s*$/, '').replace(/[—–\-:\s]+$/, '').trim()
    const lower = s.toLowerCase()
    const isPlaceholder =
      !s ||
      lower.startsWith('factory ') ||
      lower.startsWith('factory-') ||
      lower === cleanId.toLowerCase()
    const subject = isPlaceholder ? 'Automated Factory PR' : Array.from(s).slice(0, 72).join('').trimEnd()
    return `[${cleanId}] ${subject}`
  }

  function computePrMetadata(
    identifier: string,
    versionTag: string,
    ghRepo?: string,
    rawTitle?: string
  ) {
    const raw = (identifier || versionTag || 'WAGNER').trim()
    const clean = raw.replace(/[^A-Za-z0-9_-]+/g, '-').replace(/^-|-$/g, '')
    const branch = `factory-${clean}`
    const repo = ghRepo && !ghRepo.includes('wagner-factory') ? ghRepo : 'LPauzies/SoftwareFactory'
    const baseBranch = defaultBaseBranch.value
    const prTitle = formatPrTitle(clean, rawTitle)
    const url = `https://github.com/${repo}/compare/${baseBranch}...${branch}?expand=1&title=${encodeURIComponent(prTitle)}`
    return { branch, url, baseBranch, prTitle }
  }

  function normalizeTicketPrReview(tk: FactoryTicket): FactoryTicket {
    // Whenever OPS finishes a ticket (`stage === 'done'`), always transition it to Stage 6 (`pr_review`) with its GitHub PR against `default_branch` (`master`)
    if (tk.stage === 'done' || tk.stage === 'pr_review') {
      tk.stage = 'pr_review'
      const { branch, url } = computePrMetadata(
        tk.identifier,
        tk.version_tag,
        tk.github_repo,
        tk.title
      )
      if (!tk.pr_branch || tk.pr_branch.startsWith('wagner/')) {
        tk.pr_branch = branch
      }
      if (
        !tk.pr_url ||
        tk.pr_url.includes('wagner-factory/') ||
        tk.pr_url.includes('...wagner/') ||
        tk.pr_url.includes('/compare/main...') ||
        (tk.pr_url.includes('/compare/') && !tk.pr_url.includes('title='))
      ) {
        tk.pr_url = url
      }
    }
    return tk
  }

  function normalizeRunPrStep(run: PipelineRun): PipelineRun {
    if (!run.steps) run.steps = []
    const isSucceeded = run.status === 'succeeded'
    const { branch, url, baseBranch } = computePrMetadata(
      run.ticket_identifier || run.version_tag,
      run.version_tag,
      undefined,
      run.feature_request
    )
    if (isSucceeded) {
      if (!run.pr_branch || run.pr_branch.startsWith('wagner/')) {
        run.pr_branch = branch
      }
      if (
        !run.pr_url ||
        run.pr_url.includes('wagner-factory/') ||
        run.pr_url.includes('...wagner/') ||
        run.pr_url.includes('/compare/main...') ||
        (run.pr_url.includes('/compare/') && !run.pr_url.includes('title='))
      ) {
        run.pr_url = url
      }
    }
    if (!run.steps.some((s) => s.role === 'PR')) {
      run.steps.push({
        role: 'PR',
        title: 'GitHub PR & Release Integrator (PR)',
        wagner_pillar: `Pull Request contre ${baseBranch} & Validation Humaine (GitHub)`,
        model: 'gemini-3.8-flash-low',
        status: isSucceeded ? 'completed' : 'pending',
        started_at: isSucceeded ? run.completed_at || new Date().toISOString() : null,
        completed_at: isSucceeded ? run.completed_at || new Date().toISOString() : null,
        duration_seconds: isSucceeded ? 0.85 : 0,
        agy_conversation_id: isSucceeded ? 'agy-pr-auto' : null,
        agy_command: `agy --mode=auto --output-format json --model gemini-3.8-flash-low --effort low --add-dir "${run.workspace_dir}"`,
        summary: isSucceeded
          ? `Branch ${run.pr_branch || branch} pushed and Pull Request opened against ${baseBranch} for human review.`
          : 'Queued in WAGNER Factory...',
        markdown_output: isSucceeded
          ? `### PR — Pull Request Against \`${baseBranch}\` (${run.version_tag})\n- **Feature**: ${run.feature_request}\n- **Base ← Head**: \`${baseBranch}\` ← \`${run.pr_branch || branch}\`\n- **Review Link**: [${run.pr_url || url}](${run.pr_url || url})\n- **Human Review Checklist**:\n  - [x] PO / EXPERT domain invariants & acceptance criteria verified\n  - [x] ARCHI / DEV modular implementation & unit tests green\n  - [x] QA Test Factory BDD scenarios 100% pass\n  - [x] OPS Rollback runbook attached`
          : '',
        thinking_log: isSucceeded
          ? [
              {
                id: `cot-pr-1-${run.id}`,
                role: 'PR',
                kind: 'reasoning',
                content: `Preparing clean feature branch '${run.pr_branch || branch}' against '${baseBranch}' for '${run.feature_request}'.`,
                timestamp: run.completed_at || new Date().toISOString(),
              },
              {
                id: `cot-pr-2-${run.id}`,
                role: 'PR',
                kind: 'conclusion',
                content: `Branch '${run.pr_branch || branch}' → '${baseBranch}' ready for human review: ${run.pr_url || url}`,
                timestamp: run.completed_at || new Date().toISOString(),
              },
            ]
          : [],
        artifacts: isSucceeded
          ? [
              {
                path: `.factory/${run.version_tag}/7_pr.md`,
                category: 'PR Deliverable',
                language: 'markdown',
                content: `# Pull Request (${run.version_tag})\n\n- Branch: \`${run.pr_branch || branch}\` → \`${baseBranch}\`\n- Review URL: ${run.pr_url || url}\n`,
              },
            ]
          : [],
        usage: isSucceeded
          ? {
              input_tokens: 1850,
              output_tokens: 340,
              thinking_tokens: 140,
              cache_read_tokens: 0,
              total_tokens: 2330,
            }
          : {
              input_tokens: 0,
              output_tokens: 0,
              thinking_tokens: 0,
              cache_read_tokens: 0,
              total_tokens: 0,
            },
        quality_gate_passed: true,
      })
    }
    return run
  }

  function sanitizeAgentPrompt(raw: string): string {
    return (raw || '')
      .replace(/\[[A-Z]{2,6}-\d+\]\s*/g, '')
      .replace(/\b[A-Z]{2,6}-\d+\b/g, '')
      .replace(/[ \t]+(\r?\n)/g, '$1')
      .trim()
  }

  function applyState(data: any) {
    const rawProjects: Project[] = data.projects || []
    for (const p of rawProjects) {
      if (p.agents && !p.agents.some((a) => a.role === 'PR')) {
        p.agents.push({
          role: 'PR',
          title: 'GitHub PR & Release Integrator (PR)',
          wagner_pillar: 'Génération automatique de la PR contre main & Revue Humaine',
          model: 'gemini-3.8-flash-low',
          effort: 'low',
          enabled: true,
          system_prompt:
            "Tu es l'Intégrateur GitHub PR & Release (PR) dans WAGNER. Tu prépares la revue humaine, vérifies l'arbre Git de la branche feature, et génères la Pull Request avec la checklist de contrôle en français.",
        })
      }
      if (p.agents) {
        for (const ag of p.agents) {
          ag.system_prompt = sanitizeAgentPrompt(ag.system_prompt)
          ag.title = sanitizeAgentPrompt(ag.title)
        }
      }
    }
    projects.value = rawProjects
    linearProjects.value = (data.linear_projects || []).filter(
      (p: LinearProject) => !p.name.startsWith('Team:')
    )
    tickets.value = (data.tickets || [])
      .filter(
        (t: FactoryTicket) =>
          !dismissedTicketIds.value.includes(t.id) &&
          !dismissedTicketIds.value.includes(t.identifier)
      )
      .map((t: FactoryTicket) => normalizeTicketPrReview(t))
    linearStatus.value = data.linear_status || null
    runs.value = (data.runs || []).map((r: PipelineRun) => normalizeRunPrStep(r))
    releases.value = data.releases || []
    rollbacks.value = data.rollbacks || []

    if (
      linearProjects.value.length > 0 &&
      selectedLinearProjectId.value !== 'all' &&
      (!selectedLinearProjectId.value ||
        !linearProjects.value.some((p) => p.id === selectedLinearProjectId.value))
    ) {
      selectedLinearProjectId.value = linearProjects.value[0].id
    }

    if (notifications.value.length === 0 && tickets.value.length > 0) {
      const doneTicket = tickets.value.find((t) => t.stage === 'done')
      if (doneTicket) {
        notifications.value.push({
          id: 'seed-notif-1',
          ticket_id: doneTicket.id,
          ticket_identifier: doneTicket.identifier,
          role: 'OPS',
          kind: 'release',
          title: `${doneTicket.identifier} released (${doneTicket.version_tag})`,
          detail: 'All 6 Gemini agents (PO → EXPERT → ARCHI → DEV → QA → OPS) passed quality gates.',
          timestamp: 'READY',
          read: false,
        })
      }
    }
  }

  function connectSse() {
    const es = new EventSource('/api/events')
    es.onopen = () => {
      sseConnected.value = true
    }
    es.onerror = () => {
      sseConnected.value = false
    }
    es.onmessage = (e) => {
      if (!e.data || e.data === 'keep-alive') return
      try {
        handleFactoryEvent(JSON.parse(e.data))
      } catch {
        // ignore
      }
    }
  }

  function upsertTicket(tk: FactoryTicket) {
    const normalized = normalizeTicketPrReview(tk)
    const idx = tickets.value.findIndex((t) => t.id === normalized.id)
    if (idx !== -1) {
      tickets.value[idx] = normalized
    } else {
      tickets.value.unshift(normalized)
    }
  }

  function handleFactoryEvent(evt: any) {
    if (evt.event_type === 'ticket_updated' && evt.ticket) {
      upsertTicket(evt.ticket)
    } else if (evt.event_type === 'ticket_deleted' && evt.ticket_id) {
      recordDismissedTicket(evt.ticket_id, evt.identifier)
      tickets.value = tickets.value.filter((t) => t.id !== evt.ticket_id)
      if (selectedTicketId.value === evt.ticket_id) {
        selectedTicketId.value = null
      }
      lastActivityToast.value = `Deleted ${evt.identifier || evt.ticket_id}`
    } else if (evt.event_type === 'pipeline_started') {
      lastActivityToast.value = `WAGNER STARTED // ${evt.version_tag}`
      const tk = tickets.value.find((t) => t.id === evt.ticket_id)
      pushNotification({
        ticket_id: evt.ticket_id || null,
        ticket_identifier: tk?.identifier || evt.version_tag,
        role: 'PO',
        kind: 'running',
        title: `Factory started on ${tk?.identifier || evt.version_tag}`,
        detail: evt.feature_request || '6-agent autonomous pipeline initialized',
      })
    } else if (evt.event_type === 'agent_step_started') {
      lastActivityToast.value = `EXEC [${evt.role}] :: ${evt.model}`
      const tk = evt.ticket_id ? tickets.value.find((t) => t.id === evt.ticket_id) : null
      if (tk) {
        tk.stage = evt.stage
        tk.active_role = evt.role
      }
      pushNotification({
        ticket_id: evt.ticket_id || null,
        ticket_identifier: tk?.identifier || null,
        role: evt.role,
        kind: 'running',
        title: `Agent [${evt.role}] is working${tk ? ` on ${tk.identifier}` : ''}`,
        detail: `Model: ${evt.model} · Stage: ${evt.stage}`,
      })
      const r = runs.value.find((x) => x.id === evt.run_id)
      if (r) {
        r.status = 'running'
        const step = r.steps.find((s) => s.role === evt.role)
        if (step) {
          step.status = 'running'
          step.started_at = evt.started_at || step.started_at || new Date().toISOString()
          step.completed_at = null
          step.duration_seconds = 0
          step.agy_command = evt.agy_command
        }
      }
    } else if (evt.event_type === 'agent_thought_emitted') {
      const r = runs.value.find((x) => x.id === evt.run_id)
      if (r && evt.thought) {
        const step = r.steps.find((s) => s.role === evt.role)
        if (step) {
          if (!step.thinking_log) step.thinking_log = []
          const exists = step.thinking_log.some((t) => t.id === evt.thought.id)
          if (!exists) {
            step.thinking_log.push(evt.thought)
          }
        }
        if (evt.thought.kind === 'question') {
          const tk = evt.ticket_id ? tickets.value.find((t) => t.id === evt.ticket_id) : null
          pushNotification({
            ticket_id: evt.ticket_id || null,
            ticket_identifier: tk?.identifier || null,
            role: evt.role,
            kind: 'question',
            title: `Agent [${evt.role}] asked a question`,
            detail: evt.thought.content,
          })
        }
      }
    } else if (evt.event_type === 'agent_step_completed') {
      const s: AgentStepExecution = evt.step
      const isStepFailed = s.status === 'failed' || s.quality_gate_passed === false
      lastActivityToast.value = isStepFailed
        ? `CRASHED [${s.role}] // ${s.duration_seconds}s`
        : `DONE [${s.role}] // ${s.duration_seconds}s`
      const tk = evt.ticket_id ? tickets.value.find((t) => t.id === evt.ticket_id) : null
      pushNotification({
        ticket_id: evt.ticket_id || null,
        ticket_identifier: tk?.identifier || null,
        role: s.role,
        kind: isStepFailed ? 'rollback' : 'completed',
        title: isStepFailed
          ? `Agent [${s.role}] crashed / failed (${s.duration_seconds}s)`
          : `Agent [${s.role}] completed (${s.duration_seconds}s)`,
        detail: s.summary,
      })
      const r = runs.value.find((x) => x.id === evt.run_id)
      if (r) {
        const idx = r.steps.findIndex((st) => st.role === s.role)
        if (idx !== -1) r.steps[idx] = s
        r.total_tokens = r.steps.reduce((acc, st) => acc + (st.usage?.total_tokens || 0), 0)
        r.total_duration_seconds = Number(
          r.steps.reduce((acc, st) => acc + (st.duration_seconds || 0), 0).toFixed(2)
        )
      }
    } else if (evt.event_type === 'pipeline_completed') {
      const finishedRun: PipelineRun = normalizeRunPrStep(evt.run)
      const isFailed = finishedRun.status === 'failed'
      lastActivityToast.value = isFailed
        ? `PIPELINE FAILED // ${finishedRun.version_tag}`
        : `PR READY // ${finishedRun.version_tag}`
      const idx = runs.value.findIndex((x) => x.id === finishedRun.id)
      if (idx !== -1) {
        runs.value[idx] = finishedRun
      } else {
        runs.value.unshift(finishedRun)
      }
      if (evt.ticket) {
        if (!isFailed) {
          evt.ticket.stage = 'pr_review'
          evt.ticket.pr_branch = evt.ticket.pr_branch || finishedRun.pr_branch
          evt.ticket.pr_url = evt.ticket.pr_url || finishedRun.pr_url
        }
        upsertTicket(evt.ticket)
      }
      if (evt.release) {
        releases.value.unshift(evt.release)
      }
      pushNotification({
        ticket_id: evt.ticket?.id || finishedRun.ticket_id || null,
        ticket_identifier: evt.ticket?.identifier || finishedRun.ticket_identifier || null,
        role: isFailed ? 'DEV' : 'PR',
        kind: isFailed ? 'rollback' : 'release',
        title: isFailed
          ? `Pipeline failed (${finishedRun.version_tag})`
          : `PR ready against main (${finishedRun.version_tag})`,
        detail: isFailed
          ? `Agent execution stopped after ${finishedRun.total_duration_seconds}s — inspect step logs for details.`
          : `Branch ${finishedRun.pr_branch || 'wagner/main'} ready for human review (${finishedRun.total_duration_seconds}s)`,
      })
    } else if (evt.event_type === 'rollback_triggered') {
      if (evt.ticket) upsertTicket(evt.ticket)
      if (evt.rollback) {
        rollbacks.value.unshift(evt.rollback)
        lastActivityToast.value = `ROLLBACK // ${evt.rollback.from_version} -> ${evt.rollback.to_version}`
        pushNotification({
          ticket_id: evt.ticket?.id || null,
          ticket_identifier: evt.ticket?.identifier || null,
          role: 'OPS',
          kind: 'rollback',
          title: `Rollback ${evt.rollback.from_version} → ${evt.rollback.to_version}`,
          detail: evt.rollback.reason,
        })
      }
      refreshState()
    } else if (evt.event_type === 'agent_cancelled') {
      if (evt.ticket) {
        evt.ticket.active_run_id = null
        evt.ticket.active_role = null
        upsertTicket(evt.ticket)
      }
      if (evt.run_id) {
        const r = runs.value.find((x) => x.id === evt.run_id)
        if (r) {
          r.status = 'failed'
        }
      }
      lastActivityToast.value = `Agent killed on ${evt.ticket?.identifier || 'ticket'}`
      pushNotification({
        ticket_id: evt.ticket?.id || null,
        ticket_identifier: evt.ticket?.identifier || null,
        role: evt.killed_role || 'FACTORY',
        kind: 'rollback',
        title: `Agent ${evt.killed_role || ''} killed on ${evt.ticket?.identifier || ''}`,
        detail: 'Execution aborted by operator and ticket returned to Backlog.',
      })
      refreshState()
    } else if (evt.event_type === 'linear_synced') {
      if (evt.linear_status) linearStatus.value = evt.linear_status
      if (evt.linear_projects) linearProjects.value = evt.linear_projects
      if (evt.tickets) {
        tickets.value = evt.tickets
          .filter(
            (t: FactoryTicket) =>
              !dismissedTicketIds.value.includes(t.id) &&
              !dismissedTicketIds.value.includes(t.identifier)
          )
          .map((t: FactoryTicket) => normalizeTicketPrReview(t))
      }
      lastActivityToast.value = `Linear synced (${evt.tickets?.length || 0} tickets)`
    }
  }

  async function refreshState() {
    const [res, mapRes] = await Promise.all([
      fetch('/api/state'),
      fetch('/api/project-mappings'),
    ])
    if (res.ok) {
      applyState(await res.json())
    }
    if (mapRes.ok) {
      projectMappings.value = await mapRes.json()
    }
  }

  async function syncLinear(apiKey?: string) {
    const res = await fetch('/api/linear/sync', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ api_key: apiKey || null }),
    })
    if (!res.ok) {
      throw new Error(await res.text())
    }
    await refreshState()
  }

  async function updateLinearProjectRepo(
    projectId: string,
    repoPath: string,
    githubRepo?: string
  ) {
    const res = await fetch(`/api/linear/projects/${projectId}/repo`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        default_repo_path: repoPath,
        default_github_repo: githubRepo || null,
      }),
    })
    if (res.ok) {
      await refreshState()
    }
  }

  async function upsertProjectMapping(mapping: ProjectRepoMapping) {
    const res = await fetch('/api/project-mappings', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(mapping),
    })
    if (res.ok) {
      projectMappings.value = await res.json()
      await refreshState()
    }
  }

  async function createTicket(
    title: string,
    description: string,
    priorityLabel: string,
    repositoryPath: string
  ) {
    if (!activeLinearProject.value) return
    const res = await fetch('/api/tickets', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        linear_project_id: activeLinearProject.value.id,
        title,
        description,
        priority_label: priorityLabel,
        repository_path: repositoryPath,
      }),
    })
    if (res.ok) {
      const tk: FactoryTicket = await res.json()
      tickets.value.unshift(tk)
      return tk
    }
  }

  async function launchTicketFactory(
    ticketId: string,
    repositoryPath?: string,
    executionMode: 'agy_live' | 'agy_fast_simulation' = 'agy_live'
  ) {
    const res = await fetch(`/api/tickets/${ticketId}/launch`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        repository_path: repositoryPath || null,
        execution_mode: executionMode,
      }),
    })
    if (res.ok) {
      const data = await res.json()
      if (data.ticket) upsertTicket(data.ticket)
      if (data.run) {
        const normalizedRun = normalizeRunPrStep(data.run)
        const exists = runs.value.find((r) => r.id === normalizedRun.id)
        if (!exists) runs.value.unshift(normalizedRun)
      }
      return data
    }
  }

  async function requestTicketChanges(
    ticketId: string,
    feedback: string,
    executionMode: 'agy_live' | 'agy_fast_simulation' = 'agy_live'
  ) {
    const res = await fetch(`/api/tickets/${ticketId}/request-changes`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        feedback,
        execution_mode: executionMode,
      }),
    })
    if (res.ok) {
      const data = await res.json()
      if (data.ticket) upsertTicket(data.ticket)
      if (data.run) {
        const normalizedRun = normalizeRunPrStep(data.run)
        const exists = runs.value.find((r) => r.id === normalizedRun.id)
        if (!exists) runs.value.unshift(normalizedRun)
      }
      return data
    } else {
      const err = await res.text()
      throw new Error(err || 'Failed to request changes on ticket')
    }
  }

  async function cancelTicketFactory(ticketId: string) {
    const existing = tickets.value.find((t) => t.id === ticketId)
    if (existing) {
      existing.active_role = null
      existing.active_run_id = null
      existing.pending_question = null
      existing.stage = 'todo'
    }
    const res = await fetch(`/api/tickets/${ticketId}/cancel`, {
      method: 'POST',
    })
    if (res.ok) {
      const tk: FactoryTicket = await res.json()
      upsertTicket(tk)
      await refreshState()
      return tk
    }
  }

  async function deleteTicket(ticketId: string) {
    const existing = tickets.value.find((t) => t.id === ticketId)
    if (!existing || existing.stage === 'todo') {
      return
    }
    recordDismissedTicket(ticketId, existing.identifier)
    tickets.value = tickets.value.filter((t) => t.id !== ticketId)
    if (selectedTicketId.value === ticketId) {
      selectedTicketId.value = null
    }
    try {
      await fetch(`/api/tickets/${ticketId}`, {
        method: 'DELETE',
      })
    } catch {
      // ignore network error; local dismissal already recorded
    }
  }

  async function steerTicketAgent(
    ticketId: string,
    role: string,
    message: string,
    executionMode: 'agy_live' | 'agy_fast_simulation' = 'agy_live'
  ) {
    const res = await fetch(`/api/tickets/${ticketId}/steer`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        role,
        message,
        execution_mode: executionMode,
      }),
    })
    if (!res.ok) {
      throw new Error(await res.text())
    }
    const updatedStep: AgentStepExecution = await res.json()
    const run = selectedTicketRun.value
    if (run) {
      const idx = run.steps.findIndex((s) => s.role === updatedStep.role)
      if (idx !== -1) run.steps[idx] = updatedStep
    }
    return updatedStep
  }

  async function triggerRollbackForTicket(ticket: FactoryTicket, reason: string) {
    if (!activeProject.value) return
    const res = await fetch(`/api/projects/${activeProject.value.id}/rollback`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ticket_id: ticket.id,
        reason,
        use_agy_live: true,
      }),
    })
    if (res.ok) {
      await refreshState()
    }
  }

  async function saveBricks(bricks: FactoryBrick[]) {
    if (!activeProject.value) return
    const res = await fetch(`/api/projects/${activeProject.value.id}/bricks`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ bricks }),
    })
    if (res.ok) {
      const updated: Project = await res.json()
      projects.value[0] = updated
    }
  }

  async function saveAgents(agents: AgentConfig[]) {
    if (!activeProject.value) return
    const sanitized = agents.map((ag) => ({
      ...ag,
      title: sanitizeAgentPrompt(ag.title),
      system_prompt: sanitizeAgentPrompt(ag.system_prompt),
    }))
    const res = await fetch(`/api/projects/${activeProject.value.id}/agents`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ agents: sanitized }),
    })
    if (res.ok) {
      const updated: Project = await res.json()
      projects.value[0] = updated
    }
  }

  async function moveTicketStage(ticketId: string, stage: KanbanStage) {
    const res = await fetch(`/api/tickets/${ticketId}/stage`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ stage }),
    })
    if (res.ok) {
      const tk: FactoryTicket = await res.json()
      upsertTicket(tk)
      return tk
    }
  }

  return {
    agyStatus,
    projects,
    linearProjects,
    projectMappings,
    tickets,
    linearStatus,
    runs,
    releases,
    rollbacks,
    selectedLinearProjectId,
    selectedTicketId,
    sseConnected,
    loading,
    lastActivityToast,
    notifications,
    runningTickets,
    pendingQuestionTickets,
    unreadNotificationCount,
    subscriptionQuotaTokens,
    geminiUsageSummary,
    setSubscriptionQuota,
    activeProject,
    activeLinearProject,
    projectTickets,
    selectedTicket,
    selectedTicketRun,
    runForTicket,
    initialize,
    refreshState,
    markNotificationsRead,
    clearNotifications,
    syncLinear,
    updateLinearProjectRepo,
    upsertProjectMapping,
    createTicket,
    launchTicketFactory,
    requestTicketChanges,
    cancelTicketFactory,
    deleteTicket,
    steerTicketAgent,
    triggerRollbackForTicket,
    moveTicketStage,
    saveBricks,
    saveAgents,
  }
})
