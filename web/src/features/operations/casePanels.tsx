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
    key: 'operations.tasks',
    label: 'Tasks',
    audience: 'staff',
    applies: (c) =>
      ['venue_booking', 'equipment_hire', 'road_issue', 'building'].includes(
        c.module,
      ),
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
