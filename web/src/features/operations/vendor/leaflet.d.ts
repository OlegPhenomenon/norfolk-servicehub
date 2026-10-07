export interface Map { on(event: string, callback: (event: { latlng: { lat: number; lng: number } }) => void): this; remove(): void; setView(point: [number, number], zoom: number): this }
export interface Layer { addTo(map: Map): this; bindPopup(content: HTMLElement): this; remove(): void }
export function map(element: HTMLElement, options: Record<string, unknown>): Map
export function tileLayer(url: string, options: Record<string, unknown>): Layer
export function circleMarker(point: [number, number], options: Record<string, unknown>): Layer
