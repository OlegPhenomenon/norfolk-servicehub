/** Client-side story text for the demo personas (keys = `users.persona_key`). */
export const PERSONA_DESCRIPTIONS: Record<string, string> = {
  alexey: 'Hires Rawson Hall for a family celebration, applies for a building approval, orders a planning certificate, hires council equipment and reports a road problem.',
  ben: 'Applies on behalf of his company. Colleagues see the organisation’s requests only while their membership is active.',
  olga: 'Checks new requests, asks applicants for missing information and keys in phone and walk-in requests.',
  priya: 'Assesses applications, comments on drawings and holds decision authority for approvals and planning certificates.',
  tom: 'Matches payments and bank statements, issues final invoices from actual usage and decides bonds and refunds.',
  jake: 'Sees only the tasks assigned to him — hall preparation, inspections, equipment jobs, road repairs. Works on a phone.',
  helen: 'Oversees every non-confidential case, reassigns work, grants decision authority and watches the dashboard.',
  ruth: 'Handles confidential complaints and reviews, independently of the staff they concern.',
  mark: 'Configures services, prices, users and integrations — and, by design, sees no case content.',
}

/** Display order on /demo. Unknown personas are appended. */
export const PERSONA_ORDER = ['alexey', 'ben', 'olga', 'priya', 'tom', 'jake', 'helen', 'ruth', 'mark']
