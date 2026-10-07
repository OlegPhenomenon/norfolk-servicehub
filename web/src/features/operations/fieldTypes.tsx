import type { FieldComponent } from '@/featureTypes'
import { BookingSlot, EquipmentRequest, Location } from './fields'
export const fieldTypes: Record<string, FieldComponent> = {
  booking_slot: BookingSlot,
  equipment_request: EquipmentRequest,
  location: Location,
}
