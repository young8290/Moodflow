export interface CaptureAsset {
  id: string
  extension: 'png' | 'gif' | 'mp4'
  created_at: number
  bytes: number
  width: number
  height: number
  duration_ms: number
  permanent: boolean
  sha256: string
  name?: string
  kept?: boolean
}
export interface CaptureImage {
  asset: CaptureAsset
  path: string
}
export interface CaptureCrop {
  x: number
  y: number
  width: number
  height: number
}
export interface Point {
  x: number
  y: number
}
/** Coordinates are physical source pixels, independent of display DPI or CSS scaling. */
export function capturePoint(
  clientX: number,
  clientY: number,
  bounds: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>,
  width: number,
  height: number,
): Point {
  return {
    x: Math.max(0, Math.min(width, ((clientX - bounds.left) * width) / Math.max(1, bounds.width))),
    y: Math.max(0, Math.min(height, ((clientY - bounds.top) * height) / Math.max(1, bounds.height))),
  }
}
export function captureCrop(
  start: Point,
  end: Point,
  width: number,
  height: number,
  ratio: number | null,
): CaptureCrop {
  const dx = end.x >= start.x ? 1 : -1
  const dy = end.y >= start.y ? 1 : -1
  let w = Math.abs(end.x - start.x)
  let h = Math.abs(end.y - start.y)
  if (ratio) {
    h = w / ratio
    if (h > (dy > 0 ? height - start.y : start.y)) {
      h = dy > 0 ? height - start.y : start.y
      w = h * ratio
    }
  }
  const x = Math.floor(Math.min(start.x, start.x + w * dx))
  const y = Math.floor(Math.min(start.y, start.y + h * dy))
  return {
    x,
    y,
    width: Math.max(0, Math.min(width - x, Math.round(w))),
    height: Math.max(0, Math.min(height - y, Math.round(h))),
  }
}

export const CAPTURE_BATCH_LIMIT = 40

export type CaptureScope = 'all' | 'temporary' | 'kept' | 'library' | 'trash'
export type CaptureOperation = 'keep' | 'release' | 'trash' | 'restore' | 'purge' | 'export'
export type CaptureOutcome =
  | 'ready'
  | 'done'
  | 'missing'
  | 'stale'
  | 'in_use'
  | 'wrong_state'
  | 'changed_file'
  | 'failed'
  | 'destination_exists'
  | 'export_limit'
  | 'paused'

export interface CaptureSelection {
  id: string
  revision: string
}

export interface CaptureRow {
  asset: CaptureAsset
  revision: string
  scope: Exclude<CaptureScope, 'all'>
  in_use: boolean
}

interface Usage {
  count: number
  bytes: number
}

export interface CaptureCatalogPage {
  rows: CaptureRow[]
  storage: Record<
    'total' | 'temporary' | 'kept' | 'library' | 'trash' | 'expired' | 'png' | 'gif' | 'mp4',
    Usage
  > & {
    quota_bytes: number
  }
  matched: Usage
  offset: number
  limit: number
}

export interface CaptureBatchEntry extends CaptureSelection {
  status: CaptureOutcome
  bytes: number
  filename?: string
  name?: string
  created_at?: number
}

export interface CaptureBatchReport {
  entries: CaptureBatchEntry[]
  ready: number
  ready_bytes: number
  done: number
  done_bytes: number
}
