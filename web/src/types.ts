export interface ShortcutInfo {
  name: string
  repo: string
}

export interface ArtifactInfo {
  fullName: string
  repo: string
  tag: string
  digest: string
  encrypted: boolean
  version: string
  size: number
  labels?: Record<string, string>
}
