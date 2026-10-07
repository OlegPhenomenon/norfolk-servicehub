import { describe, expect, it, vi } from 'vitest'
import { renderToString } from 'react-dom/server'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import type { ServiceModule } from './api/types'
import type { CaseDetail } from './features/cases/types'
import { ResidentCasePage, StaffCasePage } from './features/cases/CasePage'
import { casePanels, panelsFor, publicNav } from './registry'
import { ToastProvider } from './ui'

// Leaflet requires a browser at import time; maps are outside this panel mounting audit.
vi.mock('./features/operations/IslandMap', () => ({ IslandMap: () => null }))

const modules: ServiceModule[] = ['generic', 'venue_booking', 'equipment_hire', 'building', 'planning_certificate', 'road_issue', 'complaint']
function detail(module: ServiceModule): CaseDetail {
  return {
    case: { id: 42, number: 'NSH-2026-000042', service_id: 1, service_name: 'Integration fixture', module, title: 'Integration fixture', status: 'in_progress', current_step: 'intake', applicant_name: 'Fictional applicant', confidential: module === 'complaint', revision: 1, created_at: '2026-10-07T00:00:00Z', submitted_at: '2026-10-07T00:00:00Z', updated_at: '2026-10-07T00:00:00Z', applicant_status_text: 'Under review', required_action: null },
    access: { kind: 'staff', can_manage: true }, step: null, steps: [],
    definition: { summary: '', outcome: '', who_can_apply: '', price_note: '', keywords: [], fields: [], documents: [], workflow: { steps: [] }, deadlines: [], pricing: [] },
    answers: {}, allowed_actions: [], required_action: null, timeline: [], assignments: [], deadlines: [], applicant_status_text: 'Under review',
  }
}

describe('cross-feature case workspaces', () => {
  it('keeps public destinations and panel keys unique, including core tabs', () => {
    const coreKeys = ['overview', 'messages', 'notes', 'assignments', 'deadlines', 'representatives', 'timeline']
    const keys = [...coreKeys, ...casePanels.map(p => p.key)]
    expect(new Set(keys).size).toBe(keys.length)
    expect(new Set(publicNav.map(n => n.to)).size).toBe(publicNav.length)
    expect(publicNav.map(n => n.label)).toEqual(['Services', 'Public notices', 'Road issues map'])
  })
  for (const module of modules) {
    for (const audience of ['staff', 'applicant'] as const) {
      it(`mounts every registered ${module} panel on the ${audience} case route`, () => {
        const d = detail(module), panels = panelsFor(d.case, audience)
        const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
        client.setQueryData(['cases', 'detail', '42'], d)
        const area = audience === 'staff' ? 'staff' : 'my'
        // Mount each feature's selected panel through the actual workspace and its ?tab= contract.
        for (const panel of panels) {
          const markup = renderToString(
            <QueryClientProvider client={client}>
              <ToastProvider>
                <MemoryRouter initialEntries={[`/${area}/cases/42?tab=${panel.key}`]}>
                  <Routes><Route path={`/${area}/cases/:id`} element={audience === 'staff' ? <StaffCasePage /> : <ResidentCasePage />} /></Routes>
                </MemoryRouter>
              </ToastProvider>
            </QueryClientProvider>,
          )
          expect(markup.match(/role="tabpanel" id="([^"]+)"/)?.[1]).toContain(`panel-${panel.key}`)
          expect(markup).toContain(panel.label)
        }
        client.clear()
      })
    }
  }
})
