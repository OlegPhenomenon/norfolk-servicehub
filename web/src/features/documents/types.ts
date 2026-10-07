export interface Comment { id: number; body: string; visibility: 'applicant' | 'internal'; author: string; created_at: string; resolved_by_version_id: number | null; request_new_version: boolean }
export interface Version { id: number; version: number; uploaded_at: string; uploader: string; note: string | null; comments: Comment[] }
export interface Document { id: number; case_id: number; title: string; category: string; visibility: string; requirement_key: string | null; versions: Version[] }
export interface Evidence { id: number; title: string; version: number }
export interface Decision { id: number; case_id: number; decision_type: string; outcome: string; reasons: string; conditions: string | null; status: string; template_id: number | null; returned_reason: string | null; issued_at: string | null; output_document_version_id: number | null; supersedes_decision_id: number | null; evidence: Evidence[] }
export interface DecisionList { items: Decision[]; authorities: string[]; staff: boolean; can_prepare: boolean; can_comment: boolean; can_upload: boolean; editable: boolean; document_requirements: {key: string; label: string}[]; revision: number; building_project_id?: number | null }
export interface Template { id: number; name: string; decision_type: string; version: number; body_template: string }
export interface Rect { page: number; x: number; y: number; w: number; h: number }
export interface Exhibition { id: number; case_id: number; title: string; summary: string; status: string; opens_at: string; closes_at: string; prepared_by: number; approved_by: number | null }
export interface Item { id: number; title: string; source_document_version_id: number; redactions_json: string; published_blob_id: number | null }
export interface ExhibitionDetail { exhibition: Exhibition; items: Item[]; revision: number }
export interface PublicDetail { exhibition: Exhibition; items: { id: number; title: string; file_url: string }[] }
