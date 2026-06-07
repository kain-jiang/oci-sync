import { useState, useEffect, useCallback } from 'react'
import type { ShortcutInfo, ArtifactInfo } from '../types'
import { getShortcuts, getArtifacts } from '../api/client'

export function useArtifacts() {
  const [shortcuts, setShortcuts] = useState<ShortcutInfo[]>([])
  const [selectedShortcut, setSelectedShortcut] = useState<ShortcutInfo | null>(null)
  const [artifacts, setArtifacts] = useState<ArtifactInfo[]>([])
  const [selectedArtifact, setSelectedArtifact] = useState<ArtifactInfo | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getShortcuts()
      .then(setShortcuts)
      .catch((e) => setError(String(e)))
  }, [])

  const loadArtifacts = useCallback(async (shortcut: ShortcutInfo) => {
    setSelectedShortcut(shortcut)
    setSelectedArtifact(null)
    setLoading(true)
    setError(null)
    try {
      const arts = await getArtifacts(shortcut.repo)
      setArtifacts(arts)
    } catch (e) {
      setError(String(e))
      setArtifacts([])
    } finally {
      setLoading(false)
    }
  }, [])

  const refresh = useCallback(async () => {
    if (selectedShortcut) {
      await loadArtifacts(selectedShortcut)
    }
  }, [selectedShortcut, loadArtifacts])

  return {
    shortcuts,
    selectedShortcut,
    setSelectedShortcut,
    artifacts,
    selectedArtifact,
    setSelectedArtifact,
    loading,
    error,
    loadArtifacts,
    refresh,
  }
}
