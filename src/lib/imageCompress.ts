/** Compress large camera/screenshot images before multipart upload (Web). */

const MAX_EDGE_PX = 2560
const COMPRESS_IF_OVER_BYTES = 1_500_000
const JPEG_QUALITY = 0.85

function isCompressibleImage(file: File): boolean {
  const mime = file.type.trim().toLowerCase()
  if (!mime.startsWith('image/')) return false
  // Keep GIF animation / SVG as-is.
  if (mime === 'image/gif' || mime === 'image/svg+xml') return false
  return true
}

/**
 * Downscale + re-encode oversized images to JPEG to speed upload.
 * Returns the original file when compression is unnecessary or fails.
 */
export async function maybeCompressImageFile(file: File): Promise<File> {
  if (!isCompressibleImage(file)) return file
  if (file.size <= COMPRESS_IF_OVER_BYTES) return file
  if (typeof createImageBitmap !== 'function' || typeof document === 'undefined') {
    return file
  }

  let bitmap: ImageBitmap | undefined
  try {
    bitmap = await createImageBitmap(file)
    const { width, height } = bitmap
    if (width <= 0 || height <= 0) return file
    const scale = Math.min(1, MAX_EDGE_PX / Math.max(width, height))
    const tw = Math.max(1, Math.round(width * scale))
    const th = Math.max(1, Math.round(height * scale))
    const canvas = document.createElement('canvas')
    canvas.width = tw
    canvas.height = th
    const ctx = canvas.getContext('2d')
    if (!ctx) return file
    ctx.drawImage(bitmap, 0, 0, tw, th)
    const blob = await new Promise<Blob | null>(resolve => {
      canvas.toBlob(resolve, 'image/jpeg', JPEG_QUALITY)
    })
    if (!blob || blob.size >= file.size) return file
    const base = file.name.replace(/\.[^.]+$/, '') || 'image'
    console.info('[imageCompress]', {
      from: file.size,
      to: blob.size,
      size: `${width}x${height}→${tw}x${th}`
    })
    return new File([blob], `${base}.jpg`, { type: 'image/jpeg', lastModified: Date.now() })
  } catch (e) {
    console.warn('[imageCompress] failed, using original', e)
    return file
  } finally {
    bitmap?.close()
  }
}
