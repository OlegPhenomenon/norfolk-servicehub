import type { CasePanel } from '@/featureTypes'
import {
  BookingPanel,
  TasksPanel,
  EquipmentPanel,
  RoadResponsePanel,
} from './Panels'
export const casePanels: CasePanel[] = [
  {
    key: 'operations.road-response',
    label: 'Road response',
    audience: 'staff',
    applies: (c) => c.module === 'road_issue',
    Component: RoadResponsePanel,
  },
  {
    key: 'operations.booking',
    label: 'Booking',
    audience: 'both',
    applies: (c) => c.module === 'venue_booking',
    Component: BookingPanel,
  },
  {
    // Any service whose frozen workflow creates field tasks (task steps), whatever its module.
    key: 'operations.tasks',
    label: 'Tasks',
    audience: 'staff',
    applies: (_c, definition) =>
      definition.workflow.steps.some((s) => s.kind === 'task'),
    Component: TasksPanel,
  },
  {
    key: 'operations.equipment',
    label: 'Equipment',
    audience: 'both',
    applies: (c) => c.module === 'equipment_hire',
    Component: EquipmentPanel,
  },
]
