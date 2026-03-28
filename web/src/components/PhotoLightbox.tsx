import { useEffect, useRef, useState, useCallback } from 'react';
import LightGallery from 'lightgallery/react';
import lgZoom from 'lightgallery/plugins/zoom';
import lgThumbnail from 'lightgallery/plugins/thumbnail';
import 'lightgallery/css/lightgallery.css';
import 'lightgallery/css/lg-zoom.css';
import 'lightgallery/css/lg-thumbnail.css';
import type { LightGallery as LightGalleryInstance } from 'lightgallery/lightgallery';
import type { InitDetail } from 'lightgallery/lg-events';

export interface PhotoItem {
  id: number;
  file_name: string;
}

interface PhotoLightboxProps {
  photos: PhotoItem[];
  openIndex: number | null;
  onClose: () => void;
}

interface PhotoInfo {
  file_name: string;
  file_path: string;
  file_size: number | null;
  width: number | null;
  height: number | null;
  format: string | null;
  taken_at: string | null;
}

const infoCache = new Map<number, PhotoInfo>();

async function fetchPhotoInfo(id: number): Promise<PhotoInfo | null> {
  if (infoCache.has(id)) return infoCache.get(id)!;
  try {
    const res = await fetch(`/api/photos/${id}`);
    if (!res.ok) return null;
    const info: PhotoInfo = await res.json();
    infoCache.set(id, info);
    return info;
  } catch { return null; }
}

function formatSize(bytes: number | null): string {
  if (bytes == null) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(dateStr: string | null): string {
  if (!dateStr) return '';
  try {
    return new Date(dateStr).toLocaleDateString(undefined, {
      year: 'numeric', month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  } catch { return dateStr; }
}

function buildSubHtml(info: PhotoInfo): string {
  const parts: string[] = [];
  parts.push(`<b>${info.file_name}</b>`);
  const meta = [
    info.format?.toUpperCase(),
    info.width && info.height ? `${info.width} × ${info.height}` : null,
    formatSize(info.file_size) || null,
  ].filter(Boolean).join(' · ');
  if (meta) parts.push(meta);
  const taken = formatDate(info.taken_at);
  if (taken) parts.push(`Taken: ${taken}`);
  parts.push(`<span style="opacity:0.5;font-size:11px">${info.file_path}</span>`);
  return `<div style="padding:8px 16px;font-size:13px;line-height:1.8">${parts.join('<br/>')}</div>`;
}

export default function PhotoLightbox({ photos, openIndex, onClose }: PhotoLightboxProps) {
  const lgRef = useRef<LightGalleryInstance | null>(null);
  const [subHtmlMap, setSubHtmlMap] = useState<Record<number, string>>({});

  const onInit = useCallback((detail: InitDetail) => {
    lgRef.current = detail.instance;
  }, []);

  // Prefetch photo info for all photos
  useEffect(() => {
    if (photos.length === 0) return;
    photos.forEach(async (p) => {
      if (subHtmlMap[p.id]) return;
      const info = await fetchPhotoInfo(p.id);
      if (info) {
        setSubHtmlMap(prev => ({ ...prev, [p.id]: buildSubHtml(info) }));
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [photos]);

  // Refresh lightGallery when subHtmlMap updates
  useEffect(() => {
    lgRef.current?.refresh();
  }, [subHtmlMap]);

  // Open gallery programmatically when openIndex changes
  useEffect(() => {
    if (openIndex == null || openIndex < 0 || openIndex >= photos.length) return;
    const timer = setTimeout(() => {
      lgRef.current?.openGallery(openIndex);
    }, 50);
    return () => clearTimeout(timer);
  }, [openIndex, photos.length]);

  if (photos.length === 0) return null;

  return (
    <LightGallery
      onInit={onInit}
      onAfterClose={onClose}
      plugins={[lgZoom, lgThumbnail]}
      download={true}
      speed={300}
      backdropDuration={200}
      hideScrollbar={true}
      closable={true}
      showCloseIcon={true}
      exThumbImage="data-thumb"
      elementClassNames="photomind-lightgallery"
    >
      {photos.map((p) => (
        <a
          key={p.id}
          href={`/api/photos/${p.id}/preview`}
          data-thumb={`/api/photos/${p.id}/thumbnail`}
          data-download-url={`/api/photos/${p.id}/file`}
          data-sub-html={subHtmlMap[p.id] || `<div style="padding:8px 16px;font-size:13px">${p.file_name}</div>`}
          style={{ display: 'none' }}
        >
          <img alt={p.file_name} src={`/api/photos/${p.id}/thumbnail`} style={{ display: 'none' }} />
        </a>
      ))}
    </LightGallery>
  );
}
