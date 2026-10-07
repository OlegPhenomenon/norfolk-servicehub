import type { ServiceDefinition, ServiceModule } from '@/api/types'
export interface ServiceCard { id: number; slug: string; name: string; category: string; module: ServiceModule; department: string; summary: string; outcome: string; price_note: string }
export interface Catalogue { items: ServiceCard[]; categories: string[] }
export interface ServiceDetail { service: ServiceCard; version_id: number; definition: ServiceDefinition; source_note: string; price_schedule_note: string; prices: { code: string; name: string; unit: string; kind: string; amount_cents: number }[] }
export interface Version { id: number; version: number; status: 'draft' | 'published' | 'retired'; definition: ServiceDefinition; source_note: string | null; source_blob_id: number | null; created_at: string; published_at: string | null }
export interface EditorData { service: ServiceCard; versions: Version[] }
