import type { DocumentRequirement } from '@/api/types'
export interface Comment { id: number; body: string; visibility: 'applicant' | 'internal'; author: string; created_at: string; resolved_by_version_id: number | null; request_new_version: boolean }
export interface Version { id: number; version: number; uploaded_at: string; uploader: string; note: string | null; comments: Comment[] }
export interface Document { id: number; case_id: number; title: string; category: string; visibility: string; requirement_key: string | null; versions: Version[] }
export interface Evidence { id: number; title: string; version: number }
export interface Decision { prepared_by: number; id: number; case_id: number; decision_type: string; outcome: string; reasons: string; conditions: string | null; status: string; template_id: number | null; returned_reason: string | null; issued_at: string | null; output_document_version_id: number | null; supersedes_decision_id: number | null; evidence: Evidence[] }
export interface DecisionList {
  allowed_decision_types: string[]; items: Decision[]; authorities: string[]; staff: boolean; can_prepare: boolean; can_comment: boolean; can_upload: boolean; editable: boolean; document_requirements: DocumentRequirement[]; revision: number; building_project_id?: number | null }
export interface Template { id: number; name: string; decision_type: string; version: number; body_template: string }
export interface Rect { page: number; x: number; y: number; w: number; h: number }
export interface Exhibition { id: number; case_id: number; title: string; summary: string; status: string; opens_at: string; closes_at: string; prepared_by: number; approved_by: number | null; terminated_at?: string | null; termination_reason?: string | null; consideration_summary?: string | null }
export interface Item { id: number; title: string; source_document_version_id: number; redactions_json: string; published_blob_id: number | null }
export interface ExhibitionDetail { exhibition: Exhibition; items: Item[]; revision: number }
export interface PublicDetail { exhibition: Exhibition; items: { id: number; title: string; file_url: string; preview_url: string }[] }
export interface FeeAssessment { id: number; version: number; method: 'schedule' | 'manual'; rule: string; inputs: { estimated_cost_cents: number | null; estimated_cost_source: string; modification_types: string[] | null; modification_types_source: string; pricing_date: string }; explanation: string; amount_cents: number; reason: string | null; assessed_by: string | null; assessed_at: string; charged_cents: number | null; adjustment: { id: number; number: string; kind: string; total_cents: number } | null }
export interface FeeView { applies: boolean; route?: string | null; schedule_note?: string; application?: { estimated_cost_cents: number | null; modification_types: string[] }; proposal?: { rule: string; amount_cents: number; explanation: string } | null; proposal_error?: string | null; assessments?: FeeAssessment[]; invoiced?: boolean; settled?: boolean; waivers?: { item_code: string; amount_cents: number; reason: string; approved_by: string }[]; can_assess?: boolean }
export interface RouteExhibition { id: number; title: string; status: string; opens_at: string | null; closes_at: string | null; terminated_at: string | null; termination_reason: string | null; consideration_summary: string | null; submissions: number; pending_submissions: number }
export interface BuildingRoute {
  route: 'project' | 'modification' | null; revision: number
  scope: { approvals: string[]; originals: number[]; confirmed: boolean } | null
  scope_history: { scope: { approvals?: string[]; originals?: number[] }; source: 'applicant' | 'staff'; reason: string; set_by: string | null; set_at: string }[]
  originals: { decision_id: number; approval_type: string; case_id: number; case_number: string | null; issued_at: string | null; in_scope: boolean; modification_decision_id: number | null }[]
  fee: FeeView; exhibition_step: boolean
  exhibition: { exhibitions: RouteExhibition[]; not_required: { reason: string; decided_at: string; decided_by: string | null } | null; block: string | null } | null
  can_scope: boolean; can_exhibit: boolean
}
export interface Chain { root_decision_id: number; approval_type: string; current_decision_id: number | null; versions: { id: number; case_id: number; decision_type: string; outcome: string; issued_at: string | null; supersedes_decision_id: number | null; output_document_version_id: number | null; state: 'current' | 'superseded' | 'refused' }[] }
export interface Submission { id: number; name: string; email: string; body: string; status: string; created_at: string; outcome: string | null; considered_at: string | null; considered_by: string | null }
