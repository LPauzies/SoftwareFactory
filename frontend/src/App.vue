<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from './i18n'
import {
  useFactoryStore,
  useAgentActivityState,
  evaluateTicketActivity,
  type FactoryTicket,
  type KanbanStage,
  type AgentNotification,
  type AgentStepExecution,
} from './store/factory'

const store = useFactoryStore()
const { locale, setLocale, t } = useI18n()

function toggleLocale() {
  setLocale(locale.value === 'fr' ? 'en' : 'fr')
}

// Theme state: 'dark' (Soft Charcoal-Black / Light Blue) | 'light' (Ice White / Light Blue)
const theme = ref<'dark' | 'light'>(
  (localStorage.getItem('wagner_theme') as 'dark' | 'light') || 'dark'
)

function toggleTheme() {
  theme.value = theme.value === 'dark' ? 'light' : 'dark'
  document.documentElement.setAttribute('data-theme', theme.value)
  localStorage.setItem('wagner_theme', theme.value)
}

// Navbar Agent Notification Center dropdown
const showNotifDropdown = ref(false)

function toggleNotifDropdown() {
  showNotifDropdown.value = !showNotifDropdown.value
  if (showNotifDropdown.value) {
    store.markNotificationsRead()
  }
}

function openTicketFromNotification(notif: AgentNotification) {
  if (notif.ticket_id) {
    store.selectedTicketId = notif.ticket_id
    drawerTab.value = 'cot'
    if (notif.role) {
      drawerStepRole.value = notif.role
    }
  }
  notif.read = true
  showNotifDropdown.value = false
}

function inspectRunningTicket(tk: FactoryTicket) {
  store.selectedTicketId = tk.id
  drawerTab.value = 'cot'
  if (tk.active_role) {
    drawerStepRole.value = tk.active_role
  }
  showNotifDropdown.value = false
}

// Slide-over drawer state when clicking a ticket
const drawerStepRole = ref<string>('PO')
const drawerTab = ref<'cot' | 'files' | 'tests'>('cot')

// Human-in-the-loop Steering / Answer Input
const steeringMessage = ref('')
const steeringLoading = ref(false)

// Modals
const showNewTicketModal = ref(false)
const newTicketTitle = ref('')
const newTicketDesc = ref('')
const newTicketPriority = ref('High')

const showLinearSyncModal = ref(false)
const linearApiKeyInput = ref(localStorage.getItem('wagner_linear_api_key') || '')
const syncingLinear = ref(false)
const linearSyncError = ref('')

const showFactorySettingsModal = ref(false)
const showUsagePopover = ref(false)

// Authentic agy CLI braille spinner frames ("⣾ ⣷ ⣯ ⣟ ⡿ ⢿ ⣻ ⣽")
const brailleFrames = ['⣾', '⣷', '⣯', '⣟', '⡿', '⢿', '⣻', '⣽']
const brailleIdx = ref(0)
const nowMs = ref(Date.now())
let brailleTimer: ReturnType<typeof setInterval> | null = null

const brailleSpinner = computed(() => brailleFrames[brailleIdx.value])

function ticketActivity(tk: FactoryTicket) {
  return evaluateTicketActivity(tk, store.runForTicket(tk), {
    nowMs: nowMs.value,
  })
}

function stepActivity(step?: AgentStepExecution | null) {
  return useAgentActivityState(
    step,
    store.selectedTicket,
    store.selectedTicketRun,
    { nowMs: nowMs.value }
  )
}

const activeStepActivity = computed(() => stepActivity(activeDrawerStep.value))

// Authentic agy CLI ASCII block bar ("█" and "░")
const asciiQuotaBlocks = computed(() => {
  const totalCells = 10
  const usedRatio = Math.min(Math.max(store.geminiUsageSummary.percentUsed / 100, 0), 1)
  const filled = Math.max(1, Math.round(usedRatio * totalCells))
  return {
    filled: '█'.repeat(filled),
    empty: '░'.repeat(Math.max(0, totalCells - filled)),
  }
})

const asciiQuotaBlocksWide = computed(() => {
  const totalCells = 20
  const usedRatio = Math.min(Math.max(store.geminiUsageSummary.percentUsed / 100, 0), 1)
  const filled = Math.max(1, Math.round(usedRatio * totalCells))
  return {
    filled: '█'.repeat(filled),
    empty: '░'.repeat(Math.max(0, totalCells - filled)),
  }
})

const quotaBarColorClass = computed(() => {
  const pct = store.geminiUsageSummary.percentUsed
  if (pct >= 85) return 'quota-crit'
  if (pct >= 60) return 'quota-warn'
  return 'quota-ok'
})

// Active model & effort display matching `store.ModelEffortDisplay` in agy CLI
const statuslineModelDisplay = computed(() => {
  const runningTk = store.runningTickets[0]
  const proj = store.activeProject
  if (runningTk && proj && runningTk.active_role) {
    const ag = proj.agents.find((a) => a.role === runningTk.active_role)
    if (ag) {
      return `${ag.model} (${ag.effort})`
    }
  }
  const defaultAg = proj?.agents?.[0]
  return defaultAg
    ? `${defaultAg.model} (${defaultAg.effort})`
    : 'gemini-3.1-pro-preview (high)'
})

const statuslineWorkspaceDisplay = computed(() => {
  const runningTk = store.runningTickets[0]
  if (runningTk?.repository_path) {
    return shortRepoPath(runningTk.repository_path)
  }
  if (store.activeLinearProject?.default_repo_path) {
    return shortRepoPath(store.activeLinearProject.default_repo_path)
  }
  return '~/Documents/SoftwareFactory'
})

onMounted(async () => {
  brailleTimer = setInterval(() => {
    brailleIdx.value = (brailleIdx.value + 1) % brailleFrames.length
    nowMs.value = Date.now()
  }, 110)
  document.documentElement.setAttribute('data-theme', theme.value)
  await store.initialize()
  // Auto-sync with Linear on page load if a key is stored on backend or in browser localStorage
  const savedKey = localStorage.getItem('wagner_linear_api_key') || undefined
  if (store.linearStatus?.has_api_key || savedKey) {
    try {
      await store.syncLinear(savedKey)
    } catch {
      // ignore initial sync error
    }
  } else if (store.linearProjects.some((p) => p.source === 'linear_api')) {
    // Prompt user to save their key once if they previously synced before key persistence was added
    showLinearSyncModal.value = true
  }
})

onUnmounted(() => {
  if (brailleTimer) {
    clearInterval(brailleTimer)
  }
})

watch(
  () => store.selectedTicket,
  (tk) => {
    if (tk?.active_role) {
      drawerStepRole.value = tk.active_role
    }
  }
)

// Auto-follow active role in the Chain-of-Thought viewer when a ticket is running
watch(
  () => store.selectedTicket?.active_role,
  (activeRole) => {
    if (activeRole) {
      drawerStepRole.value = activeRole
    }
  }
)

const columns: Array<{
  id: KanbanStage
  index: string
  title: string
  subtitle: string
  blueAccent?: boolean
}> = [
  {
    id: 'todo',
    index: 'Stage 1',
    title: 'Backlog',
    subtitle: 'Linear issues queued for Wagner',
  },
  {
    id: 'design',
    index: 'Stage 2',
    title: 'Design & Spec',
    subtitle: 'PO · Business Expert · Architect',
    blueAccent: true,
  },
  {
    id: 'coding',
    index: 'Stage 3',
    title: 'Implementation',
    subtitle: 'Developer agent in repository',
    blueAccent: true,
  },
  {
    id: 'testing',
    index: 'Stage 4',
    title: 'Test Factory',
    subtitle: 'QA BDD & integration verification',
    blueAccent: true,
  },
  {
    id: 'pr_review',
    index: 'Stage 5',
    title: 'PR & Human Review',
    subtitle: 'GitHub PR against master for review',
    blueAccent: true,
  },
  {
    id: 'done',
    index: 'Stage 6',
    title: 'Released & Run',
    subtitle: 'Deployed with rollback runbook',
  },
]

function extractTicketNumber(identifier: string): number {
  const match = identifier.match(/(\d+)\s*$/)
  return match ? parseInt(match[1], 10) : Number.MAX_SAFE_INTEGER
}

function compareTicketsByNumberAsc(a: FactoryTicket, b: FactoryTicket): number {
  const numA = extractTicketNumber(a.identifier)
  const numB = extractTicketNumber(b.identifier)
  if (numA !== numB) {
    return numA - numB
  }
  return a.identifier.localeCompare(b.identifier, undefined, { numeric: true })
}

function formatTicketCreatedAt(tk: FactoryTicket): string {
  const raw = tk.created_at || tk.updated_at
  if (!raw) return ''
  const d = new Date(raw)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleDateString('fr-FR', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  })
}

function formatTime(raw?: string | null): string {
  if (!raw) return ''
  const d = new Date(raw)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleTimeString('fr-FR', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function formatDurationSec(seconds?: number | null): string {
  if (seconds === undefined || seconds === null) return '0s'
  if (seconds < 60) return `${Number(seconds.toFixed(1))}s`
  const m = Math.floor(seconds / 60)
  const s = Math.round(seconds % 60)
  return `${m}m ${s}s`
}

function currentRunTotalDuration(run?: any): string {
  if (!run) return '0s'
  let total: number = run.total_duration_seconds || 0
  const runningStep = run.steps?.find((s: any) => s.status === 'running')
  if (runningStep) {
    const act = stepActivity(runningStep)
    total += act.elapsedSeconds
  }
  return formatDurationSec(total)
}

function ticketsForColumn(colId: KanbanStage): FactoryTicket[] {
  if (colId === 'done') {
    return store.projectTickets
      .filter((t) => t.stage === 'done' || t.stage === 'rolled_back')
      .slice()
      .sort(compareTicketsByNumberAsc)
  }
  return store.projectTickets
    .filter((t) => t.stage === colId)
    .slice()
    .sort(compareTicketsByNumberAsc)
}

function shortRepoPath(path: string): string {
  const parts = path.split('/').filter(Boolean)
  if (parts.length <= 2) return path
  return `…/${parts.slice(-2).join('/')}`
}

function shortRoleCode(role: string): string {
  if (!role) return ''
  const upper = role.toUpperCase()
  if (upper === 'EXPERT') return 'EXP'
  if (upper === 'ARCHI') return 'ARC'
  return upper
}

const activeDrawerStep = computed(() => {
  const run = store.selectedTicketRun
  if (!run) return null
  return (
    run.steps.find((s) => s.role === drawerStepRole.value) ||
    run.steps[0] ||
    null
  )
})

const allRunArtifacts = computed(() => {
  const run = store.selectedTicketRun
  if (!run) return []
  return run.steps.flatMap((s) =>
    s.artifacts.map((a) => ({ ...a, role: s.role }))
  )
})

function isTicketQueued(tk: FactoryTicket): boolean {
  if (tk.active_role) return false
  if (!tk.active_run_id) return false
  const run = store.runs.find((r) => r.id === tk.active_run_id)
  return (
    !!run &&
    (run.status === 'queued' ||
      (run.status === 'running' &&
        run.steps.every((s) => s.status === 'pending' || s.status === 'skipped')))
  )
}

async function handleQuickLaunch(ticket: FactoryTicket, evt?: Event) {
  if (evt) evt.stopPropagation()
  store.selectedTicketId = ticket.id
  drawerTab.value = 'cot'
  await store.launchTicketFactory(
    ticket.id,
    ticket.repository_path,
    'agy_live'
  )
}

async function handleLaunchFromDrawer() {
  if (!store.selectedTicket) return
  drawerTab.value = 'cot'
  await store.launchTicketFactory(
    store.selectedTicket.id,
    store.selectedTicket.repository_path,
    'agy_live'
  )
}

async function handleKillAgent(ticket: FactoryTicket, evt?: Event) {
  if (evt) evt.stopPropagation()
  await store.cancelTicketFactory(ticket.id)
}

async function handleSteerAgent() {
  if (!store.selectedTicket || !activeDrawerStep.value || !steeringMessage.value.trim()) {
    return
  }
  steeringLoading.value = true
  try {
    const msg = steeringMessage.value.trim()
    steeringMessage.value = ''
    await store.steerTicketAgent(
      store.selectedTicket.id,
      activeDrawerStep.value.role,
      msg,
      'agy_live'
    )
  } finally {
    steeringLoading.value = false
  }
}

async function handleRollbackTicket(ticket: FactoryTicket, evt?: Event) {
  if (evt) evt.stopPropagation()
  await store.triggerRollbackForTicket(
    ticket,
    `Rollback triggered from WAGNER Kanban card ${ticket.identifier}`
  )
}

async function handleDeleteTicket(ticket: FactoryTicket, evt?: Event) {
  if (evt) evt.stopPropagation()
  if (ticket.stage === 'todo') return
  await store.deleteTicket(ticket.id)
}

async function handleCreateTicket() {
  if (!newTicketTitle.value.trim()) return
  await store.createTicket(
    newTicketTitle.value.trim(),
    newTicketDesc.value.trim() || 'Created in WAGNER Autonomous Software Factory',
    newTicketPriority.value,
    store.activeLinearProject?.default_repo_path || ''
  )
  newTicketTitle.value = ''
  newTicketDesc.value = ''
  showNewTicketModal.value = false
}

async function handleSyncLinear() {
  syncingLinear.value = true
  linearSyncError.value = ''
  try {
    const trimmedKey = linearApiKeyInput.value.trim()
    if (trimmedKey) {
      localStorage.setItem('wagner_linear_api_key', trimmedKey)
    }
    await store.syncLinear(trimmedKey || undefined)
    showLinearSyncModal.value = false
  } catch (err: any) {
    linearSyncError.value = err?.message || 'Failed to sync with Linear API'
  } finally {
    syncingLinear.value = false
  }
}

async function handleToggleBrick(idx: number) {
  if (!store.activeProject) return
  const cloned = JSON.parse(JSON.stringify(store.activeProject.bricks))
  cloned[idx].enabled = !cloned[idx].enabled
  await store.saveBricks(cloned)
}

async function handleSaveAgentModels() {
  if (!store.activeProject) return
  const cloned = JSON.parse(JSON.stringify(store.activeProject.agents))
  await store.saveAgents(cloned)
}
</script>

<template>
  <div class="app-shell">
    <!-- Top Architectural Header -->
    <header class="topbar">
      <div class="brand">
        <img
          src="/wagner-mark.svg"
          alt="Wagner"
          class="wagner-logo-img"
        />
        <div class="brand-title">
          <span>Wagner</span>
        </div>
      </div>

      <div class="topbar-right">
        <!-- Live Activity Telemetry -->
        <span v-if="store.lastActivityToast" class="tag-box active-blue">
          <span class="square-dot"></span>
          {{ store.lastActivityToast }}
        </span>

        <!-- Antigravity Subprocess Status -->
        <span class="tag-box" :class="store.agyStatus?.available ? 'active-blue' : ''">
          <span class="square-dot"></span>
          agy --mode=auto
        </span>

        <!-- Agent Activity & Notification Button in Navbar -->
        <div class="notif-wrapper">
          <button
            class="notif-btn"
            :class="{
              open: showNotifDropdown,
              'has-active':
                store.runningTickets.length > 0 || store.pendingQuestionTickets.length > 0,
            }"
            @click="toggleNotifDropdown"
          >
            <span
              class="notif-dot"
              :class="{
                question: store.pendingQuestionTickets.length > 0,
                live:
                  store.pendingQuestionTickets.length === 0 &&
                  store.runningTickets.length > 0,
              }"
            ></span>
            <span>
              {{
                store.runningTickets.length > 0
                  ? `Agents (${store.runningTickets.length} active)`
                  : store.pendingQuestionTickets.length > 0
                    ? `Agents (${store.pendingQuestionTickets.length} question)`
                    : 'Agents'
              }}
            </span>
            <span
              v-if="
                store.unreadNotificationCount > 0 ||
                store.runningTickets.length > 0 ||
                store.pendingQuestionTickets.length > 0
              "
              class="notif-count-badge"
            >
              {{
                store.runningTickets.length > 0
                  ? store.runningTickets.length
                  : store.unreadNotificationCount || store.pendingQuestionTickets.length
              }}
            </span>
          </button>

          <!-- Agent Notification Dropdown Panel -->
          <div v-if="showNotifDropdown" class="notif-dropdown">
            <div class="notif-header">
              <span class="field-label" style="color: var(--accent-primary)">
                Agent Activity &amp; Notifications
              </span>
              <div style="display: flex; gap: 0.35rem">
                <button
                  class="btn"
                  style="padding: 0.2rem 0.5rem; font-size: 0.72rem"
                  @click="store.clearNotifications()"
                >
                  Clear
                </button>
                <button
                  class="btn"
                  style="padding: 0.2rem 0.5rem; font-size: 0.72rem"
                  @click="showNotifDropdown = false"
                >
                  Close
                </button>
              </div>
            </div>

            <!-- Section 1: Currently Active / Working Agents -->
            <div class="notif-section-title">
              Active Agents ({{ store.runningTickets.length }} running ·
              {{ store.pendingQuestionTickets.length }} questions)
            </div>
            <div
              v-if="
                store.runningTickets.length === 0 &&
                store.pendingQuestionTickets.length === 0
              "
              style="
                padding: 0.75rem 0.9rem;
                font-size: 0.78rem;
                color: var(--text-muted);
                border-bottom: 1px solid var(--border-grid);
              "
            >
              All 6 agents (PO, Expert, Architect, Dev, QA, Ops) are currently idle.
            </div>
            <div v-else class="notif-list" style="max-height: 160px">
              <div
                v-for="tk in [...store.runningTickets, ...store.pendingQuestionTickets.filter(q => !q.active_role)]"
                :key="tk.id"
                class="notif-item"
                :class="{ question: !!tk.pending_question, unread: !!tk.active_role }"
                @click="inspectRunningTicket(tk)"
              >
                <div style="display: flex; justify-content: space-between; align-items: center">
                  <span style="font-size: 0.76rem; font-weight: 600; color: var(--accent-primary)">
                    {{ tk.identifier }} ·
                    {{ tk.active_role ? `Running ${tk.active_role}` : 'Waiting for answer' }}
                  </span>
                  <div style="display: flex; gap: 0.35rem; align-items: center">
                    <button
                      class="btn btn-danger"
                      style="font-size: 0.68rem; padding: 0.14rem 0.45rem"
                      @click="handleKillAgent(tk, $event)"
                    >
                      Kill
                    </button>
                    <span class="tag-box" style="font-size: 0.68rem; padding: 0.1rem 0.4rem">
                      Inspect →
                    </span>
                  </div>
                </div>
                <div style="font-size: 0.8rem; font-weight: 600">{{ tk.title }}</div>
                <div
                  v-if="tk.pending_question"
                  style="font-size: 0.75rem; color: var(--status-warn)"
                >
                  Question: {{ tk.pending_question }}
                </div>
              </div>
            </div>

            <!-- Section 2: Recent Agent Event Feed -->
            <div class="notif-section-title">
              Recent Activity ({{ store.notifications.length }})
            </div>
            <div class="notif-list">
              <div
                v-if="store.notifications.length === 0"
                style="padding: 1rem 0.9rem; font-size: 0.78rem; color: var(--text-muted)"
              >
                No recent agent activity. Launch a ticket to follow live agent steps.
              </div>
              <div
                v-for="n in store.notifications"
                :key="n.id"
                class="notif-item"
                :class="{ unread: !n.read, question: n.kind === 'question' }"
                @click="openTicketFromNotification(n)"
              >
                <div style="display: flex; justify-content: space-between; align-items: center">
                  <span style="font-size: 0.74rem; font-weight: 600; color: var(--accent-primary)">
                    {{ n.role || 'Factory' }} · {{ n.ticket_identifier || '' }}
                  </span>
                  <span style="font-size: 0.7rem; color: var(--text-muted)">
                    {{ n.timestamp }}
                  </span>
                </div>
                <div style="font-size: 0.8rem; font-weight: 600">{{ n.title }}</div>
                <div style="font-size: 0.75rem; color: var(--text-secondary)">
                  {{ n.detail }}
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Factory Bricks & Models -->
        <button class="btn" @click="showFactorySettingsModal = true">
          Settings
        </button>

        <!-- Language Switcher (FR / EN) -->
        <button
          class="btn"
          @click="toggleLocale"
          :title="locale === 'fr' ? 'Switch to English' : 'Passer en Français'"
          style="min-width: 4rem; font-weight: 600"
        >
          {{ locale === 'fr' ? '🇫🇷 FR' : '🇬🇧 EN' }}
        </button>

        <!-- Theme Switcher -->
        <button class="btn btn-primary" @click="toggleTheme">
          {{ theme === 'dark' ? 'Dark' : 'Light' }}
        </button>
      </div>
    </header>

    <!-- Linear Project Selector Bar -->
    <section class="project-bar">
      <div class="bar-group">
        <span class="field-label">Linear Project</span>
        <select
          v-model="store.selectedLinearProjectId"
          class="select"
          style="min-width: 240px; font-weight: 600"
        >
          <option value="all">📂 Tous les projets (All Projects)</option>
          <option
            v-for="lp in store.linearProjects"
            :key="lp.id"
            :value="lp.id"
          >
            {{ lp.key }} — {{ lp.name }}
          </option>
        </select>
      </div>

      <div class="bar-group">
        <button class="btn btn-primary" @click="showNewTicketModal = true">
          + New Ticket
        </button>
      </div>
    </section>

    <!-- 5-Column Exposed Grid Kanban Board -->
    <main class="kanban-wrapper">
      <div v-for="col in columns" :key="col.id" class="kanban-col">
        <div class="col-header" :class="{ 'blue-top': col.blueAccent }">
          <div>
            <div class="col-index">{{ col.index }}</div>
            <div class="col-title">{{ col.title }}</div>
            <div class="col-sub">{{ col.subtitle }}</div>
          </div>
          <span class="col-count">{{ ticketsForColumn(col.id).length }}</span>
        </div>

        <div class="col-body">
          <div
            v-for="tk in ticketsForColumn(col.id)"
            :key="tk.id"
            class="ticket-card"
            :class="{
              active: store.selectedTicketId === tk.id,
              running: ticketActivity(tk).isRunning,
              crashed: ticketActivity(tk).isCrashed,
              waiting: ticketActivity(tk).state === 'waiting',
            }"
            @click="store.selectedTicketId = tk.id"
          >
            <div
              v-if="ticketActivity(tk).isRunning"
              class="card-activity-glow"
              aria-hidden="true"
            ></div>

            <div class="ticket-top">
              <span class="ticket-id">{{ tk.identifier }}</span>
              <div style="display: flex; align-items: center; gap: 0.35rem">
                <span class="prio-badge" :class="tk.priority_label">
                  {{ tk.priority_label }}
                </span>
                <button
                  v-if="tk.stage !== 'todo'"
                  class="btn btn-danger"
                  style="padding: 0.08rem 0.42rem; font-size: 0.72rem; line-height: 1.1"
                  title="Supprimer cette carte"
                  @click="handleDeleteTicket(tk, $event)"
                >
                  ×
                </button>
              </div>
            </div>

            <div class="ticket-title">{{ tk.title }}</div>
            <div class="ticket-desc">{{ tk.description }}</div>

            <!-- Target repository badge & creation date in Backlog -->
            <div style="display: flex; align-items: center; justify-content: space-between; gap: 0.4rem">
              <div class="repo-tag" :title="tk.repository_path">
                {{ shortRepoPath(tk.repository_path) }}
              </div>
              <span
                v-if="tk.stage === 'todo' && formatTicketCreatedAt(tk)"
                class="mono"
                style="font-size: 0.68rem; color: var(--text-muted); white-space: nowrap"
                :title="`Créé le ${formatTicketCreatedAt(tk)}`"
              >
                📅 {{ formatTicketCreatedAt(tk) }}
              </span>
            </div>

            <!-- GitHub Pull Request against master badge & link for Human Review -->
            <!-- Only show when ticket is NOT actively running and is in pr_review or done -->
            <div
              v-if="!tk.active_role && tk.pr_url && (tk.stage === 'pr_review' || tk.stage === 'done')"
              class="card-pr-box"
              @click.stop
            >
              <div class="card-pr-branch mono">
                <span class="branch-icon">⎇</span> {{ tk.pr_branch || `factory-${tk.identifier}` }}
                <span class="branch-arrow">→</span> {{ store.projectMappings[0]?.default_branch || 'master' }}
              </div>
              <a
                :href="tk.pr_url"
                target="_blank"
                rel="noopener noreferrer"
                class="btn-pr-action"
              >
                <span>Review PR on GitHub</span>
                <span class="pr-arrow">↗</span>
              </a>
            </div>

            <!-- Pending Question Alert if Agent asked a question -->
            <div
              v-if="tk.pending_question"
              class="card-paused-widget"
            >
              <div class="paused-widget-top">
                <span class="paused-icon">⚠</span>
                <span class="paused-title">En pause — Question</span>
              </div>
              <div class="paused-text">“{{ tk.pending_question }}”</div>
            </div>

            <!-- Live running agent indicator + Kill button -->
            <div v-if="tk.active_role" class="card-running-widget">
              <div class="running-widget-header">
                <div class="running-agent-info">
                  <span class="agent-spinner" aria-hidden="true"></span>
                  <span class="running-pulse-dot" aria-hidden="true"></span>
                  <span class="running-role-pill">Agent {{ tk.active_role }}</span>
                  <span
                    v-if="ticketActivity(tk).elapsedSeconds > 0"
                    class="running-live-timer mono"
                  >
                    {{ ticketActivity(tk).elapsedSeconds }}s
                  </span>
                </div>
                <button
                  class="btn-kill-compact"
                  title="Interrompre l'agent"
                  @click="handleKillAgent(tk, $event)"
                >
                  Interrompre
                </button>
              </div>
              <div v-if="ticketActivity(tk).startedAt" class="running-widget-meta mono">
                <span>🕒 Démarré à {{ formatTime(ticketActivity(tk).startedAt) }}</span>
              </div>
            </div>

            <!-- Mini Pipeline Steps Grid (6-step proportional tracker) -->
            <div
              v-if="store.runForTicket(tk)?.steps?.length"
              class="card-pipeline-grid"
              :title="`Pipeline ${store.runForTicket(tk)?.version_tag} — ${store.runForTicket(tk)?.total_duration_seconds}s totales`"
            >
              <div
                v-for="s in store.runForTicket(tk)!.steps"
                :key="s.role"
                class="card-pipeline-chip"
                :class="[
                  stepActivity(s).state,
                  { active: tk.active_role === s.role }
                ]"
                :title="`${s.role}: ${stepActivity(s).isRunning ? `en cours (${stepActivity(s).elapsedSeconds}s)` : s.duration_seconds > 0 ? `terminé en ${s.duration_seconds}s` : s.status}${s.started_at ? ` · démarré à ${formatTime(s.started_at)}` : ''}`"
              >
                <span class="chip-role">
                  <span
                    v-if="stepActivity(s).isRunning"
                    class="running-pulse-dot chip-pulse-dot"
                    aria-hidden="true"
                  ></span>
                  {{ shortRoleCode(s.role) }}
                </span>
                <span v-if="stepActivity(s).isRunning" class="chip-status-sub mono">
                  {{ stepActivity(s).elapsedSeconds }}s
                </span>
                <span v-else-if="s.status === 'completed' || s.status === 'succeeded'" class="chip-status-sub mono">
                  {{ s.duration_seconds > 0 ? `${s.duration_seconds}s` : '✓' }}
                </span>
                <span v-else-if="stepActivity(s).isCrashed" class="chip-status-sub">
                  💥
                </span>
              </div>
            </div>

            <!-- Crashed / Failed agent indicator on card -->
            <div
              v-if="ticketActivity(tk).isCrashed"
              class="card-crashed-widget"
              :title="ticketActivity(tk).errorMessage || ''"
            >
              <div class="crashed-widget-left">
                <span class="crashed-icon">💥</span>
                <span class="crashed-label">Agent {{ ticketActivity(tk).failedRole || 'Pipeline' }} planté</span>
              </div>
              <button
                class="btn-retry-compact"
                @click="handleQuickLaunch(tk, $event)"
              >
                🔄 Relancer
              </button>
            </div>

            <!-- Queued in FIFO execution queue indicator -->
            <div v-else-if="isTicketQueued(tk)" class="card-queued-widget">
              <span class="queued-text">⏳ En file d'attente...</span>
              <button
                class="btn-kill-compact"
                @click="handleKillAgent(tk, $event)"
              >
                Annuler
              </button>
            </div>

            <!-- Rolled back indicator -->
            <div
              v-else-if="tk.stage === 'rolled_back'"
              class="card-rolled-back-badge"
            >
              ⚠ Rolled back par l'agent OPS
            </div>

            <!-- Card Action Buttons -->
            <button
              v-if="((tk.stage === 'todo' || tk.stage === 'rolled_back') || (!tk.active_role && !isTicketQueued(tk) && tk.stage !== 'done' && tk.stage !== 'pr_review')) && !ticketActivity(tk).isCrashed"
              class="btn btn-launch"
              @click="handleQuickLaunch(tk, $event)"
            >
              {{ tk.stage === 'todo' ? 'Launch Wagner' : 'Relaunch' }}
            </button>

            <div
              v-else-if="tk.stage === 'done' || tk.stage === 'pr_review'"
              style="display: flex; gap: 0.4rem; margin-top: 0.35rem"
            >
              <button
                class="btn"
                style="flex: 1; font-size: 0.75rem; padding: 0.32rem"
                @click.stop="store.selectedTicketId = tk.id"
              >
                Inspect
              </button>
              <button
                class="btn btn-danger"
                style="font-size: 0.75rem; padding: 0.32rem 0.55rem"
                @click="handleRollbackTicket(tk, $event)"
              >
                Rollback
              </button>
            </div>
          </div>

          <div
            v-if="!ticketsForColumn(col.id).length"
            style="
              text-align: center;
              padding: 2rem 0.5rem;
              color: var(--text-muted);
              font-size: 0.78rem;
            "
          >
            No tickets
          </div>
        </div>
      </div>
    </main>

    <!-- ===================================================================== -->
    <!-- AGY CLI STATUSLINE (StatusLineModel.RenderBuiltIn + /usage TUI)       -->
    <!-- ===================================================================== -->
    <div class="agy-statusline-wrapper">
      <!-- Interactive `/usage` TUI Popover (UsageModel.View) -->
      <div v-if="showUsagePopover" class="agy-usage-popover">
        <div class="agy-usage-header">
          <span>/usage · Gemini Code Assist &amp; Antigravity Telemetry</span>
          <button class="agy-usage-close" @click="showUsagePopover = false">
            [esc]
          </button>
        </div>

        <div class="agy-usage-section">
          <div class="agy-usage-title">PLAN</div>
          <div class="agy-usage-row">
            <span>Your Plan: <strong>Google AI Pro (agy --mode=auto)</strong></span>
            <select
              :value="store.subscriptionQuotaTokens"
              class="agy-select"
              title="Subscription Quota Tier"
              @change="
                store.setSubscriptionQuota(
                  Number(($event.target as HTMLSelectElement).value)
                )
              "
            >
              <option :value="250000">Quota: 250k tokens</option>
              <option :value="500000">Quota: 500k tokens</option>
              <option :value="1000000">Quota: 1M tokens</option>
              <option :value="5000000">Quota: 5M tokens</option>
            </select>
          </div>
        </div>

        <div class="agy-usage-section">
          <div class="agy-usage-title">MODEL QUOTA</div>
          <div class="agy-usage-row">
            <span class="agy-ascii-bar" :class="quotaBarColorClass">
              <span class="filled">{{ asciiQuotaBlocksWide.filled }}</span
              ><span class="empty">{{ asciiQuotaBlocksWide.empty }}</span>
            </span>
            <span>
              <strong>{{ store.geminiUsageSummary.percentUsed }}%</strong> used ·
              <span class="agy-quota-ok">Quota available</span>
            </span>
          </div>
          <div class="agy-usage-sub">
            {{ store.geminiUsageSummary.total.toLocaleString() }} /
            {{ store.geminiUsageSummary.quota.toLocaleString() }} tokens · Refreshes in 3h 42m
          </div>
        </div>

        <div class="agy-usage-section">
          <div class="agy-usage-title">TOKEN BREAKDOWN</div>
          <div class="agy-token-grid">
            <div>
              <span class="agy-dim">↑ Input</span>
              <strong>{{ store.geminiUsageSummary.input.toLocaleString() }}</strong>
            </div>
            <div>
              <span class="agy-dim">↓ Output</span>
              <strong>{{ store.geminiUsageSummary.output.toLocaleString() }}</strong>
            </div>
            <div>
              <span class="agy-dim">✦ Thinking (CoT)</span>
              <strong>{{ store.geminiUsageSummary.thinking.toLocaleString() }}</strong>
            </div>
            <div>
              <span class="agy-dim">⚡ Cache</span>
              <strong>{{ store.geminiUsageSummary.cache.toLocaleString() }}</strong>
            </div>
          </div>
        </div>

        <div v-if="store.agyStatus" class="agy-usage-footer">
          <span>{{ store.agyStatus.version }}</span>
          <span>
            {{ store.agyStatus.brain_sessions_count || 0 }} conversations ·
            {{ store.agyStatus.brain_steps_count || 0 }} turns
          </span>
        </div>
      </div>

      <!-- Built-In 1-Line AGY Terminal Statusline -->
      <footer class="agy-statusline">
        <!-- Left: Mode Label + Agent Phase + Workspace + VCS + Shortcut Hint -->
        <div class="agy-sl-left">
          <span class="agy-mode-pill" title="Agent Mode: Auto (--mode=auto)">
            Auto
          </span>

          <span class="agy-sep">·</span>

          <!-- Agent state ("main › working" / "waiting" / "idle") -->
          <span
            v-if="store.runningTickets.some((t) => t.pending_question)"
            class="agy-state waiting"
            @click=" inspectRunningTicket(store.runningTickets[0]) "
          >
            ⚠ main › waiting for answer ({{ store.runningTickets[0]?.active_role }})
          </span>
          <span
            v-else-if="store.runningTickets.length"
            class="agy-state working"
            @click="inspectRunningTicket(store.runningTickets[0])"
          >
            <span class="agy-spinner">{{ brailleSpinner }}</span>
            main › working ({{ store.runningTickets[0]?.identifier }} ·
            {{ store.runningTickets[0]?.active_role }})
          </span>
          <span v-else class="agy-state idle">
            main › idle
          </span>

          <span class="agy-sep">·</span>

          <span class="agy-workspace" :title="statuslineWorkspaceDisplay">
            {{ statuslineWorkspaceDisplay }}
            <span class="agy-branch">({{ store.runningTickets[0] ? `factory-${store.runningTickets[0].identifier}*` : `${store.projectMappings[0]?.default_branch || 'master'}*` }})</span>
          </span>

          <span class="agy-sep hide-sm">·</span>

          <span
            class="agy-hint hide-sm"
            title="Click to open Factory Bricks & Agent Settings"
            @click="showFactorySettingsModal = true"
          >
            ? for shortcuts
          </span>
        </div>

        <!-- Right: Subagents · ↑ Input ↓ Output · ASCII Quota Bar (/usage) · Model (Effort) -->
        <div class="agy-sl-right">
          <button
            class="agy-sl-btn"
            :class="{ active: store.runningTickets.length > 0 }"
            title="Active subagents — click to toggle /agents"
            @click="toggleNotifDropdown"
          >
            {{ store.runningTickets.length }} subagent(s) · /agents
          </button>

          <span class="agy-sep">·</span>

          <span
            class="agy-tokens"
            title="Prompt Input (↑) / Completion Output (↓) / CoT Thinking (✦) tokens"
            @click="showUsagePopover = !showUsagePopover"
          >
            ↑ {{ store.geminiUsageSummary.input.toLocaleString() }}
            ↓ {{ store.geminiUsageSummary.output.toLocaleString() }}
            <span class="agy-cot-inline hide-sm">
              ✦ {{ store.geminiUsageSummary.thinking.toLocaleString() }}
            </span>
          </span>

          <span class="agy-sep">·</span>

          <button
            class="agy-sl-btn agy-quota-trigger"
            title="Gemini Subscription Quota — click to toggle /usage"
            @click="showUsagePopover = !showUsagePopover"
          >
            <span class="agy-ascii-bar" :class="quotaBarColorClass">
              <span class="filled">{{ asciiQuotaBlocks.filled }}</span
              ><span class="empty">{{ asciiQuotaBlocks.empty }}</span>
            </span>
            <span class="agy-quota-pct">{{ store.geminiUsageSummary.percentUsed }}%</span>
            <span class="agy-cmd-tag">/usage</span>
          </button>

          <span class="agy-sep">·</span>

          <span
            class="agy-model-badge"
            title="Active Model & Reasoning Effort"
            @click="showFactorySettingsModal = true"
          >
            {{ statuslineModelDisplay }}
            <span class="agy-plan-tag">PRO</span>
          </span>
        </div>
      </footer>
    </div>

    <!-- ===================================================================== -->
    <!-- SLIDE-OVER TICKET & CHAIN-OF-THOUGHT (CoT) INSPECTOR DRAWER           -->
    <!-- ===================================================================== -->
    <div
      v-if="store.selectedTicket"
      class="drawer-backdrop"
      @click.self="store.selectedTicketId = null"
    >
      <aside class="drawer">
        <div class="drawer-header">
          <div>
            <div style="display: flex; align-items: center; gap: 0.5rem">
              <span class="ticket-id" style="font-size: 0.9rem">
                {{ store.selectedTicket.identifier }}
              </span>
              <span class="prio-badge" :class="store.selectedTicket.priority_label">
                {{ store.selectedTicket.priority_label }}
              </span>
            </div>
            <h2 style="font-size: 1.05rem; margin-top: 0.35rem; font-weight: 700">
              {{ store.selectedTicket.title }}
            </h2>
            <p style="font-size: 0.8rem; color: var(--text-secondary); margin-top: 0.2rem">
              {{ store.selectedTicket.description }}
            </p>
          </div>
          <div style="display: flex; gap: 0.45rem; align-items: center">
            <button
              v-if="store.selectedTicket.stage !== 'todo'"
              class="btn btn-danger"
              title="Supprimer cette carte"
              @click="handleDeleteTicket(store.selectedTicket, $event)"
            >
              Delete
            </button>
            <button class="btn" @click="store.selectedTicketId = null">Close</button>
          </div>
        </div>

        <div class="drawer-body">
          <!-- GitHub PR against master Banner for Human Review -->
          <div
            v-if="store.selectedTicket.pr_url || store.selectedTicketRun?.pr_url"
            class="drawer-pr-banner"
          >
            <div>
              <div class="field-label" style="color: var(--status-ok)">
                GitHub Pull Request Ready for Human Review (PR Agent)
              </div>
              <div class="mono" style="font-size: 0.78rem; margin-top: 0.2rem">
                Base: <strong>{{ store.projectMappings[0]?.default_branch || 'master' }}</strong> ← Head:
                <strong>{{
                  store.selectedTicket.pr_branch ||
                  store.selectedTicketRun?.pr_branch ||
                  `factory-${store.selectedTicket.identifier}`
                }}</strong>
              </div>
            </div>
            <a
              :href="store.selectedTicket.pr_url || store.selectedTicketRun?.pr_url || '#'"
              target="_blank"
              rel="noopener noreferrer"
              class="btn btn-primary"
              style="text-decoration: none"
            >
              Review PR on GitHub ↗
            </a>
          </div>

          <!-- Target Repository & Launch Control inside Drawer -->
          <div
            style="
              background: var(--bg-card);
              border: 1px solid var(--border-grid);
              padding: 0.8rem;
              display: flex;
              align-items: center;
              justify-content: space-between;
              gap: 0.75rem;
              flex-wrap: wrap;
            "
          >
            <div>
              <div class="field-label">
                Target Repository Workspace (<code class="mono">wagner.toml</code>)
              </div>
              <div class="mono" style="font-size: 0.78rem; color: var(--text-secondary); margin-top: 0.2rem">
                {{ store.selectedTicket.repository_path }}
              </div>
            </div>
            <div style="display: flex; gap: 0.5rem">
              <button
                class="btn btn-primary"
                :disabled="!!store.selectedTicket.active_role || isTicketQueued(store.selectedTicket)"
                @click="handleLaunchFromDrawer"
              >
                {{
                  store.selectedTicket.active_role
                    ? `Running ${store.selectedTicket.active_role}...`
                    : isTicketQueued(store.selectedTicket)
                      ? '⏳ Queued (waiting for active task)...'
                      : store.selectedTicketRun && store.selectedTicketRun.status === 'failed'
                        ? '🔄 Relancer le pipeline (Retry)'
                        : store.selectedTicket.stage === 'todo'
                          ? 'Launch Wagner in Repository'
                          : '🔄 Re-run Wagner Pipeline'
                }}
              </button>
              <button
                v-if="store.selectedTicket.active_role || isTicketQueued(store.selectedTicket)"
                class="btn btn-danger"
                @click="handleKillAgent(store.selectedTicket)"
              >
                {{ store.selectedTicket.active_role ? 'Kill Agent' : 'Cancel Queue' }}
              </button>
            </div>
          </div>

          <!-- 6-Role Pipeline & Live Chain of Thought Inspector -->
          <div
            v-if="store.selectedTicketRun"
            style="display: flex; flex-direction: column; gap: 0.85rem"
          >
            <div
              style="
                display: flex;
                justify-content: space-between;
                align-items: center;
                flex-wrap: wrap;
                gap: 0.5rem;
              "
            >
              <div style="display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap">
                <span
                  v-if="store.selectedTicketRun.created_at"
                  class="field-label mono"
                  style="font-size: 0.74rem; opacity: 0.9"
                >
                  🚀 Lancé à {{ formatTime(store.selectedTicketRun.created_at) }}
                </span>
                <span class="field-label mono" style="font-size: 0.74rem; font-weight: 600">
                  ⏱ Durée totale : {{ currentRunTotalDuration(store.selectedTicketRun) }}
                </span>
                <span class="field-label" style="font-size: 0.74rem">
                  · {{ store.selectedTicketRun.total_tokens.toLocaleString() }} tokens
                </span>
              </div>
              <div style="display: flex; gap: 0.35rem">
                <button
                  class="btn"
                  :class="{ 'btn-primary': drawerTab === 'cot' }"
                  @click="drawerTab = 'cot'"
                >
                  Chain of Thought &amp; Steer
                </button>
                <button
                  class="btn"
                  :class="{ 'btn-primary': drawerTab === 'files' }"
                  @click="drawerTab = 'files'"
                >
                  Repository Files ({{ allRunArtifacts.length }})
                </button>
                <button
                  class="btn"
                  :class="{ 'btn-primary': drawerTab === 'tests' }"
                  @click="drawerTab = 'tests'"
                >
                  Tests &amp; Release Notes
                </button>
              </div>
            </div>

            <!-- 6-Role Selector Grid -->
            <div class="step-pills">
              <div
                v-for="(step, sIdx) in store.selectedTicketRun.steps"
                :key="step.role"
                class="step-pill"
                :class="[
                  step.status,
                  stepActivity(step).state,
                  { active: drawerStepRole === step.role },
                ]"
                @click="drawerStepRole = step.role"
              >
                <div
                  v-if="stepActivity(step).isRunning"
                  class="card-activity-glow"
                  aria-hidden="true"
                ></div>
                <div class="step-pill-header">
                  <span
                    v-if="stepActivity(step).isRunning"
                    class="agent-spinner step-pill-spinner"
                    aria-hidden="true"
                  ></span>
                  <span
                    v-if="stepActivity(step).isRunning"
                    class="running-pulse-dot chip-pulse-dot"
                    aria-hidden="true"
                  ></span>
                  <span
                    v-else-if="stepActivity(step).isCrashed"
                    class="step-status-icon crashed"
                    aria-hidden="true"
                  >
                    💥
                  </span>
                  <span
                    v-else-if="stepActivity(step).state === 'waiting'"
                    class="step-status-icon waiting"
                    aria-hidden="true"
                  >
                    ⚠
                  </span>
                  <span
                    v-else-if="stepActivity(step).state === 'completed'"
                    class="step-status-icon completed"
                    aria-hidden="true"
                  >
                    ✓
                  </span>
                  <span style="font-weight: 600">{{ sIdx + 1 }}. {{ step.role }}</span>
                  <span
                    v-if="step.started_at"
                    class="step-pill-time-chip mono"
                    :title="`Démarré à ${formatTime(step.started_at)}`"
                  >
                    {{ formatTime(step.started_at) }}
                  </span>
                </div>
                <div class="step-pill-sub mono">
                  <template v-if="stepActivity(step).isRunning">
                    ⚡ en cours · {{ stepActivity(step).elapsedSeconds }}s
                  </template>
                  <template v-else-if="stepActivity(step).isCrashed">
                    💥 échoué · {{ step.duration_seconds || stepActivity(step).elapsedSeconds }}s
                  </template>
                  <template v-else-if="stepActivity(step).state === 'waiting'">
                    ⚠ en pause · {{ stepActivity(step).elapsedSeconds }}s
                  </template>
                  <template v-else-if="step.status === 'succeeded' || step.status === 'completed'">
                    ✓ terminé en {{ step.duration_seconds }}s
                  </template>
                  <template v-else-if="step.status === 'skipped'">
                    ⊘ ignoré
                  </template>
                  <template v-else>
                    ⏳ en attente
                  </template>
                </div>
              </div>
            </div>

            <!-- ============================================================= -->
            <!-- TAB 01: CHAIN OF THOUGHT (CoT) + OPERATOR STEERING + OUTPUT   -->
            <!-- ============================================================= -->
            <template v-if="drawerTab === 'cot' && activeDrawerStep">
              <!-- Live Step Liveness & Crash Diagnostic Banner -->
              <div
                class="step-activity-banner"
                :class="activeStepActivity.state"
              >
                <div class="step-activity-main">
                  <span
                    v-if="activeStepActivity.isRunning"
                    class="agent-spinner"
                    aria-hidden="true"
                  ></span>
                  <span
                    v-if="activeStepActivity.isRunning"
                    class="running-pulse-dot"
                    aria-hidden="true"
                  ></span>
                  <span v-else-if="activeStepActivity.isCrashed" aria-hidden="true">
                    💥
                  </span>
                  <span v-else-if="activeStepActivity.state === 'waiting'" aria-hidden="true">
                    ⚠
                  </span>
                  <span v-else-if="activeStepActivity.state === 'completed'" aria-hidden="true">
                    ✓
                  </span>
                  <span v-else aria-hidden="true">⏳</span>

                  <div>
                    <div class="step-activity-title">
                      <template v-if="activeStepActivity.isRunning">
                        Agent {{ activeDrawerStep.role }} en cours d'exécution ({{ activeStepActivity.elapsedSeconds }}s écoulées)
                      </template>
                      <template v-else-if="activeStepActivity.isCrashed">
                        Agent {{ activeDrawerStep.role }} a planté / échoué après {{ activeDrawerStep.duration_seconds || activeStepActivity.elapsedSeconds }}s
                      </template>
                      <template v-else-if="activeStepActivity.state === 'waiting'">
                        Agent {{ activeDrawerStep.role }} en pause — en attente de votre réponse ({{ activeStepActivity.elapsedSeconds }}s)
                      </template>
                      <template v-else-if="activeStepActivity.state === 'completed'">
                        Étape {{ activeDrawerStep.role }} terminée avec succès en {{ activeDrawerStep.duration_seconds }}s
                      </template>
                      <template v-else>
                        Étape {{ activeDrawerStep.role }} en attente d'exécution
                      </template>
                    </div>
                    <div class="step-activity-detail">
                      <span v-if="activeDrawerStep.started_at" class="step-time-badge mono">
                        🕒 Démarré à {{ formatTime(activeDrawerStep.started_at) }}
                      </span>
                      <span v-if="activeDrawerStep.completed_at" class="step-time-badge mono">
                        🏁 Terminé à {{ formatTime(activeDrawerStep.completed_at) }}
                      </span>
                      <span v-if="activeStepActivity.isRunning" class="step-time-badge mono">
                        ⏱ En direct : {{ activeStepActivity.elapsedSeconds }}s
                      </span>
                      <span v-else-if="activeDrawerStep.duration_seconds > 0" class="step-time-badge mono">
                        ⏱ Durée d'exécution : {{ activeDrawerStep.duration_seconds }}s
                      </span>
                      <span v-if="activeStepActivity.isRunning && activeStepActivity.lastSignalAgeSeconds !== null" class="step-time-badge mono">
                        📡 Dernier signal il y a {{ activeStepActivity.lastSignalAgeSeconds }}s
                      </span>
                      <span
                        v-if="activeStepActivity.isStalled"
                        class="step-time-badge mono"
                        style="color: var(--status-warn); border: 1px solid rgba(240, 178, 50, 0.45)"
                      >
                        ⚠ Sans signal &gt; 60s
                      </span>
                    </div>
                    <div
                      v-if="activeStepActivity.isCrashed && activeStepActivity.errorMessage"
                      class="step-activity-detail crashed-detail"
                    >
                      {{ activeStepActivity.errorMessage }}
                    </div>
                    <div
                      v-if="activeStepActivity.isCrashed"
                      style="margin-top: 0.45rem"
                    >
                      <button
                        class="btn btn-launch"
                        style="font-size: 0.74rem; padding: 0.3rem 0.65rem; width: auto;"
                        @click="handleLaunchFromDrawer"
                      >
                        🔄 Relancer l'agent {{ activeDrawerStep.role }} / Relancer le pipeline
                      </button>
                    </div>
                  </div>
                </div>

                <span
                  v-if="activeStepActivity.isRunning"
                  class="step-live-wave"
                  aria-hidden="true"
                >
                  <span></span><span></span><span></span>
                </span>
              </div>

              <!-- Live Chain of Thought Stream -->
              <div>
                <div
                  style="
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    margin-bottom: 0.35rem;
                  "
                >
                  <span class="field-label" style="color: var(--accent-primary)">
                    Chain of Thought — {{ activeDrawerStep.role }} ({{ activeDrawerStep.title }})
                  </span>
                  <span style="font-size: 0.75rem; color: var(--text-secondary)">
                    {{ activeDrawerStep.model }} · {{ activeDrawerStep.duration_seconds }}s
                  </span>
                </div>

                <div class="cot-console">
                  <div
                    v-if="!activeDrawerStep.thinking_log || !activeDrawerStep.thinking_log.length"
                    style="color: var(--text-muted)"
                  >
                    Waiting for agent reasoning stream...
                  </div>
                  <div
                    v-for="cot in activeDrawerStep.thinking_log"
                    :key="cot.id"
                    class="cot-line"
                    :class="cot.kind"
                  >
                    <span class="cot-kind">{{ cot.kind }}</span>
                    <span>{{ cot.content }}</span>
                  </div>
                </div>
              </div>

              <!-- Pending Question Gate Alert if Agent is Paused Waiting for User Answer -->
              <div
                v-if="store.selectedTicket.pending_question"
                style="
                  padding: 0.75rem 0.9rem;
                  border-radius: 8px;
                  border: 1px solid var(--status-warn);
                  background: rgba(240, 178, 50, 0.14);
                  display: flex;
                  flex-direction: column;
                  gap: 0.25rem;
                "
              >
                <div style="font-size: 0.76rem; font-weight: 700; color: var(--status-warn)">
                  Pipeline Paused — Agent {{ store.selectedTicket.active_role || activeDrawerStep.role }} is waiting for your answer before advancing to the next step:
                </div>
                <div style="font-size: 0.86rem; font-weight: 600; color: var(--text-primary)">
                  “{{ store.selectedTicket.pending_question }}”
                </div>
              </div>

              <!-- Operator Intervention / Answer Input Bar -->
              <div class="steer-bar">
                <input
                  v-model="steeringMessage"
                  class="input"
                  :placeholder="
                    store.selectedTicket.pending_question
                      ? 'Type your answer to unblock the agent and proceed to the next step...'
                      : 'Answer agent question or steer (e.g. \'Use Postgres advisory lock with 15s timeout\')...'
                  "
                  @keyup.enter="handleSteerAgent"
                />
                <button
                  class="btn btn-primary"
                  :disabled="steeringLoading || !steeringMessage.trim()"
                  @click="handleSteerAgent"
                >
                  {{
                    steeringLoading
                      ? 'Sending...'
                      : store.selectedTicket.pending_question
                        ? `Answer & Resume (${activeDrawerStep.role})`
                        : `Steer ${activeDrawerStep.role}`
                  }}
                </button>
              </div>

              <!-- Agent Deliverable Output -->
              <div>
                <div
                  style="
                    display: flex;
                    justify-content: space-between;
                    margin-bottom: 0.35rem;
                  "
                >
                  <span class="field-label">
                    Deliverable — {{ activeDrawerStep.role }}
                  </span>
                  <span style="font-size: 0.72rem; color: var(--text-muted)">
                    {{ activeDrawerStep.agy_command }}
                  </span>
                </div>
                <pre class="code-box">{{
                  activeDrawerStep.markdown_output || activeDrawerStep.summary
                }}</pre>
              </div>
            </template>

            <!-- ============================================================= -->
            <!-- TAB 02: GENERATED REPOSITORY FILES                            -->
            <!-- ============================================================= -->
            <div
              v-else-if="drawerTab === 'files'"
              style="display: flex; flex-direction: column; gap: 0.75rem"
            >
              <div
                v-for="art in allRunArtifacts"
                :key="art.path"
                style="
                  background: var(--bg-card);
                  border: 1px solid var(--border-grid);
                  padding: 0.75rem;
                "
              >
                <div
                  style="
                    display: flex;
                    justify-content: space-between;
                    margin-bottom: 0.4rem;
                    font-size: 0.78rem;
                  "
                >
                  <span style="color: var(--accent-primary); font-weight: 600">
                    {{ art.path }}
                  </span>
                  <span class="tag-box">{{ art.role }} · {{ art.category }}</span>
                </div>
                <pre class="code-box" style="max-height: 180px">{{ art.content }}</pre>
              </div>
            </div>

            <!-- ============================================================= -->
            <!-- TAB 03: CONNECTED TEST FACTORY & LIVING RELEASE NOTES         -->
            <!-- ============================================================= -->
            <div
              v-else-if="drawerTab === 'tests'"
              style="display: flex; flex-direction: column; gap: 0.85rem"
            >
              <div>
                <div class="field-label" style="margin-bottom: 0.3rem">
                  Connected Test Factory (Gherkin BDD Scenarios)
                </div>
                <pre class="code-box" style="max-height: 180px">{{
                  store.selectedTicketRun.test_report?.gherkin_feature ||
                  'Waiting for QA Agent...'
                }}</pre>
              </div>

              <div>
                <div class="field-label" style="margin-bottom: 0.3rem">
                  Release Notes &amp; Rollback Runbook
                </div>
                <pre class="code-box" style="max-height: 180px">{{
                  store.selectedTicketRun.living_docs?.release_notes ||
                  'Waiting for Ops Agent...'
                }}</pre>
              </div>
            </div>
          </div>

          <div
            v-else
            style="
              padding: 2rem;
              text-align: center;
              color: var(--text-secondary);
              background: var(--bg-card);
              border: 1px solid var(--border-grid);
              font-size: 0.82rem;
            "
          >
            Ticket is queued in Backlog.<br />
            Click <strong>"Launch Wagner in Repository"</strong> above to stream the 7-agent Chain
            of Thought inside <code>{{ store.selectedTicket.repository_path }}</code>.
          </div>
        </div>
      </aside>
    </div>

    <!-- ===================================================================== -->
    <!-- MODAL 1: CREATE NEW TICKET                                            -->
    <!-- ===================================================================== -->
    <div
      v-if="showNewTicketModal"
      class="modal-backdrop"
      @click.self="showNewTicketModal = false"
    >
      <div class="modal-card">
        <div class="field-label" style="color: var(--accent-primary)">
          New Linear Ticket — {{ store.activeLinearProject?.key }} ·
          {{ store.activeLinearProject?.name }}
        </div>
        <input
          v-model="newTicketTitle"
          class="input"
          placeholder="Ticket title (e.g. Add webhook signature verification)"
        />
        <textarea
          v-model="newTicketDesc"
          class="textarea"
          rows="3"
          placeholder="Acceptance criteria & business rules..."
        ></textarea>
        <div style="display: flex; gap: 0.6rem; align-items: center">
          <span class="field-label">Priority</span>
          <select v-model="newTicketPriority" class="select">
            <option value="Urgent">Urgent</option>
            <option value="High">High</option>
            <option value="Medium">Medium</option>
            <option value="Low">Low</option>
          </select>
        </div>
        <div style="display: flex; justify-content: flex-end; gap: 0.5rem">
          <button class="btn" @click="showNewTicketModal = false">Cancel</button>
          <button class="btn btn-primary" @click="handleCreateTicket">
            Create Ticket
          </button>
        </div>
      </div>
    </div>

    <!-- ===================================================================== -->
    <!-- MODAL 2: CONNECT & SYNC LINEAR API                                    -->
    <!-- ===================================================================== -->
    <div
      v-if="showLinearSyncModal"
      class="modal-backdrop"
      @click.self="showLinearSyncModal = false"
    >
      <div class="modal-card">
        <div class="field-label" style="color: var(--accent-primary)">
          Linear Integration
        </div>
        <p style="font-size: 0.82rem; color: var(--text-secondary)">
          Enter your Linear Personal API Key (<code>lin_api_...</code>) to sync live
          Linear projects and issues onto the Wagner Kanban board.
        </p>
        <input
          v-model="linearApiKeyInput"
          type="password"
          class="input"
          placeholder="lin_api_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
        />
        <div
          v-if="linearSyncError"
          style="font-size: 0.78rem; color: var(--status-danger)"
        >
          {{ linearSyncError }}
        </div>
        <div style="display: flex; justify-content: flex-end; gap: 0.5rem">
          <button class="btn" @click="showLinearSyncModal = false">Cancel</button>
          <button
            class="btn btn-primary"
            :disabled="syncingLinear"
            @click="handleSyncLinear"
          >
            {{ syncingLinear ? 'Syncing...' : 'Sync Linear Workspace' }}
          </button>
        </div>
      </div>
    </div>

    <!-- ===================================================================== -->
    <!-- MODAL 3: FACTORY BRICKS & GEMINI MODELS SETTINGS                      -->
    <!-- ===================================================================== -->
    <div
      v-if="showFactorySettingsModal && store.activeProject"
      class="modal-backdrop"
      @click.self="showFactorySettingsModal = false"
    >
      <div class="modal-card" style="width: 700px">
        <div style="display: flex; justify-content: space-between; align-items: center">
          <span class="field-label" style="color: var(--accent-primary)">
            Modular Bricks &amp; Gemini Role Models
          </span>
          <button class="btn" @click="showFactorySettingsModal = false">Close</button>
        </div>

        <div class="field-label">
          Active Factory Bricks
        </div>
        <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 0.5rem">
          <label
            v-for="(brick, bIdx) in store.activeProject.bricks"
            :key="brick.kind"
            style="
              background: var(--bg-card);
              border: 1px solid var(--border-grid);
              padding: 0.55rem 0.75rem;
              display: flex;
              align-items: center;
              justify-content: space-between;
              cursor: pointer;
              font-size: 0.81rem;
            "
          >
            <span>{{ brick.name }}</span>
            <input
              type="checkbox"
              :checked="brick.enabled"
              @change="handleToggleBrick(bIdx)"
            />
          </label>
        </div>

        <div class="field-label" style="margin-top: 0.4rem">
          Gemini Model per Agent Role (<code>agy --mode=auto --model</code>)
        </div>
        <div style="display: flex; flex-direction: column; gap: 0.45rem">
          <div
            v-for="agent in store.activeProject.agents"
            :key="agent.role"
            style="
              display: grid;
              grid-template-columns: 85px 1fr 220px 100px;
              gap: 0.5rem;
              align-items: center;
              font-size: 0.78rem;
            "
          >
            <span class="mono" style="font-weight: 700; color: var(--accent-primary)">
              {{ agent.role }}
            </span>
            <span>{{ agent.title }}</span>
            <select v-model="agent.model" class="select mono" @change="handleSaveAgentModels">
              <option
                v-for="m in store.agyStatus?.models || []"
                :key="m.id"
                :value="m.id"
              >
                {{ m.id }}
              </option>
            </select>
            <select v-model="agent.effort" class="select mono" @change="handleSaveAgentModels">
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
          </div>
        </div>
        <div class="field-label" style="margin-top: 0.4rem">
          Project Repositories &amp; Base Branch (Fixed in <code>wagner.toml</code>)
        </div>
        <div style="display: flex; flex-direction: column; gap: 0.35rem">
          <div
            v-for="m in store.projectMappings"
            :key="m.linear_project"
            class="mono"
            style="
              background: var(--bg-card);
              border: 1px solid var(--border-grid);
              padding: 0.5rem 0.7rem;
              font-size: 0.76rem;
              display: grid;
              grid-template-columns: 150px 1fr 190px 80px;
              gap: 0.6rem;
              align-items: center;
            "
          >
            <span style="color: var(--accent-primary); font-weight: 700">
              {{ m.linear_project }}
            </span>
            <span style="color: var(--text-secondary)" :title="m.local_path">
              {{ m.local_path }}
            </span>
            <span style="color: var(--status-ok)">
              github.com/{{ m.github_repo }}
            </span>
            <span style="color: var(--text-muted); text-align: right">
              base: {{ m.default_branch || 'master' }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
