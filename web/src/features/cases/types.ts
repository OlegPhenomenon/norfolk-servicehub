import type { Answers, CaseSummary, ServiceDefinition, StepDef } from '@/api/types'
import type { StepState } from '@/ui/Steps'
export interface RequestSummary extends CaseSummary { applicant_status_text: string; required_action: { message_id: number; body: string; document_version_id?: number | null } | null; intake_channel?: string; applicant_email?: string; applicant_phone?: string }
export interface CaseList { items: RequestSummary[]; total: number; page: number; page_size: number }
export interface Assignment { id: number; user_id: number; name: string; role: string; reason: string; assigned_at: string; ended_at: string | null; ended_reason: string | null; assigned_by: string | null }
export interface Deadline { id: number; label: string; due_at: string; status: string; text: string; pause_days_used: number; max_pause_days: number | null }
export interface CaseDetail { case: RequestSummary; access: { kind: string; can_manage?: boolean }; step: { def: StepDef; index: number; total: number } | null; steps: { key: string; label: string; state: StepState }[]; definition: ServiceDefinition; answers: Answers; allowed_actions: string[]; guard_reason?: string | null; required_action: RequestSummary['required_action']; timeline: { id: number; at: string; summary: string; actor_name: string | null }[]; assignments?: Assignment[]; deadlines: Deadline[]; applicant_status_text: string }
export interface DraftDetail { case: RequestSummary; definition: ServiceDefinition; answers: Answers }
