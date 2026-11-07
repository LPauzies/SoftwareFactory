import { ref, computed } from 'vue'

export type Locale = 'fr' | 'en'

const savedLocale = (localStorage.getItem('wagner_locale') as Locale) || 'fr'
export const currentLocale = ref<Locale>(savedLocale)

export function setLocale(locale: Locale) {
  currentLocale.value = locale
  localStorage.setItem('wagner_locale', locale)
}

export const translations = {
  fr: {
    appName: 'WAGNER',
    appSubtitle: 'Usine Logicielle Autonome Multi-Agents',
    syncLinear: 'Synchroniser Linear',
    syncing: 'Synchronisation...',
    connected: 'Connecté',
    offline: 'Hors-ligne',
    tickets: 'Tickets',
    releases: 'Releases',
    rollbacks: 'Rollbacks',
    systemHealth: 'Santé Système',
    kanban: {
      backlog: 'Backlog Produit',
      poSpec: 'Spécification PO',
      expertValidation: 'Validation Métier',
      architecture: 'Architecture',
      development: 'Développement',
      testFactory: 'Test Factory (QA)',
      deploymentSre: 'Déploiement SRE',
      prReview: 'Revue PR & Merge',
      released: 'En Production'
    },
    actions: {
      launch: 'Lancer',
      killAgent: 'Arrêter Agent',
      deleteTicket: 'Supprimer',
      viewPr: 'Revue PR sur GitHub',
      operatorAnswer: 'Répondre',
      rollback: 'Rollback Immédiat'
    },
    agents: {
      po: 'Product Owner (PO)',
      expert: 'Expert Métier',
      archi: 'Architecte Logiciel',
      dev: 'Développeur Craft',
      qa: 'Test Factory (QA)',
      ops: 'DevSecOps & SRE',
      pr: 'Intégrateur GitHub PR'
    }
  },
  en: {
    appName: 'WAGNER',
    appSubtitle: 'Autonomous Multi-Agent Software Factory',
    syncLinear: 'Sync Linear',
    syncing: 'Syncing...',
    connected: 'Connected',
    offline: 'Offline',
    tickets: 'Tickets',
    releases: 'Releases',
    rollbacks: 'Rollbacks',
    systemHealth: 'System Health',
    kanban: {
      backlog: 'Product Backlog',
      poSpec: 'PO Specification',
      expertValidation: 'Domain Validation',
      architecture: 'Architecture',
      development: 'Development',
      testFactory: 'Test Factory (QA)',
      deploymentSre: 'SRE Deployment',
      prReview: 'PR Review & Merge',
      released: 'Production Released'
    },
    actions: {
      launch: 'Launch',
      killAgent: 'Stop Agent',
      deleteTicket: 'Delete',
      viewPr: 'Review PR on GitHub',
      operatorAnswer: 'Answer',
      rollback: 'Instant Rollback'
    },
    agents: {
      po: 'Product Owner (PO)',
      expert: 'Domain Expert',
      archi: 'Software Architect',
      dev: 'Craft Developer',
      qa: 'Test Factory (QA)',
      ops: 'DevSecOps & SRE',
      pr: 'GitHub PR Integrator'
    }
  }
}

export function useI18n() {
  const t = computed(() => translations[currentLocale.value])
  return {
    locale: currentLocale,
    setLocale,
    t
  }
}
