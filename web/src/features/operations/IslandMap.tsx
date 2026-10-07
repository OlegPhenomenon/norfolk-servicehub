import { useEffect, useRef } from 'react'
import * as L from './vendor/leaflet'
import './vendor/leaflet.css'
export interface MapPoint {
  lat: number
  lng: number
  label: string
  completed?: boolean
}
export function IslandMap({
  points,
  onPin,
  label = 'Norfolk Island map',
}: {
  points: MapPoint[]
  onPin?: (lat: number, lng: number) => void
  label?: string
}) {
  const container = useRef<HTMLDivElement>(null)
  const map = useRef<L.Map | null>(null)
  const pin = useRef(onPin)
  useEffect(() => {
    pin.current = onPin
  }, [onPin])
  useEffect(() => {
    if (!container.current) return
    const m = L.map(container.current, {
      center: [-29.04, 167.95],
      zoom: 13,
      scrollWheelZoom: false,
      maxBounds: [
        [-29.16, 167.89],
        [-28.97, 168.02],
      ],
    })
    L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', {
      maxZoom: 19,
      attribution:
        '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
    }).addTo(m)
    m.on('click', (e) =>
      pin.current?.(
        Number(e.latlng.lat.toFixed(6)),
        Number(e.latlng.lng.toFixed(6)),
      ),
    )
    map.current = m
    return () => {
      map.current = null
      m.remove()
    }
  }, [])
  useEffect(() => {
    if (!map.current) return
    const css = getComputedStyle(document.documentElement)
    const markers = points.map((p) => {
      const content = document.createElement('span')
      content.textContent = p.label
      return L.circleMarker([p.lat, p.lng], {
        radius: 9,
        color:
          css
            .getPropertyValue(p.completed ? '--color-pine' : '--color-primary')
            .trim() || 'currentColor',
        fillOpacity: 0.8,
      })
        .bindPopup(content)
        .addTo(map.current!)
    })
    return () => markers.forEach((m) => m.remove())
  }, [points])
  return (
    <div
      ref={container}
      role="region"
      aria-label={label}
      className="relative z-0 h-72 w-full rounded-lg border border-line"
    />
  )
}
