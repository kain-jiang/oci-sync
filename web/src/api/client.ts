import type { ShortcutInfo, ArtifactInfo } from '../types'

const BASE = '/api'

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init)
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error || `HTTP ${res.status}`)
  }
  return res.json()
}

export async function getShortcuts(): Promise<ShortcutInfo[]> {
  return request<ShortcutInfo[]>(`${BASE}/shortcuts`)
}

export async function getArtifacts(repo: string): Promise<ArtifactInfo[]> {
  return request<ArtifactInfo[]>(`${BASE}/artifacts?repo=${encodeURIComponent(repo)}`)
}

export async function pushArtifact(
  repo: string,
  tag: string,
  files: File[],
  passphrase?: string,
  labels?: string[],
): Promise<{ success: boolean; ref: string; size: number; encrypted: boolean }> {
  const form = new FormData()
  form.set('repo', repo)
  form.set('tag', tag)
  if (passphrase) form.set('passphrase', passphrase)
  if (labels?.length) form.set('labels', JSON.stringify(labels))
  for (const f of files) {
    form.append('files', f, f.webkitRelativePath || f.name)
  }
  return request(`${BASE}/push`, { method: 'POST', body: form })
}

export async function pullArtifact(
  repo: string,
  tag: string,
  passphrase?: string,
): Promise<Blob> {
  const res = await fetch(`${BASE}/pull`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ repo, tag, passphrase }),
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error || `HTTP ${res.status}`)
  }
  return res.blob()
}

export async function deleteArtifact(repo: string, tag: string): Promise<void> {
  await request(`${BASE}/delete`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ repo, tag }),
  })
}
