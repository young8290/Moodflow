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
