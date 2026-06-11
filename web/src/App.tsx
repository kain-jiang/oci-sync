import { useState, useCallback } from 'react'
import { useArtifacts } from './hooks/useArtifacts'
import { Sidebar } from './components/Sidebar'
import { Toolbar } from './components/Toolbar'
import { ArtifactTable } from './components/ArtifactTable'
import { DetailPanel } from './components/DetailPanel'
import { PushDialog } from './components/PushDialog'
import { PullDialog } from './components/PullDialog'
import { DeleteDialog } from './components/DeleteDialog'
import { Toast } from './components/Toast'
import { pushArtifact, pullArtifact, deleteArtifact } from './api/client'

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ['KiB', 'MiB', 'GiB', 'TiB']
  let v = n
  let u = -1
  do {
    v /= 1024
    u++
  } while (v >= 1024 && u < units.length - 1)
  return `${v.toFixed(1)} ${units[u]}`
}

type DialogType = 'push' | 'pull' | 'delete' | null

export default function App() {
  const {
    shortcuts,
    selectedShortcut,
    artifacts,
    selectedArtifact,
    setSelectedArtifact,
    loading,
    error,
    loadArtifacts,
    refresh,
  } = useArtifacts()

  const [dialog, setDialog] = useState<DialogType>(null)
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null)
  const [pullProgress, setPullProgress] = useState<{ loaded: number; total: number } | null>(null)

  const showToast = useCallback((message: string, type: 'success' | 'error') => {
    setToast({ message, type })
  }, [])

  const handlePush = useCallback(
    async (tag: string, files: FileList, passphrase: string, labelsStr: string) => {
      if (!selectedShortcut) return
      setDialog(null)
      const labels = labelsStr
        ? labelsStr.split(',').map((s) => s.trim()).filter(Boolean)
        : []
      try {
        await pushArtifact(selectedShortcut.repo, tag, Array.from(files), passphrase || undefined, labels)
        showToast(`Pushed ${selectedShortcut.name}:${tag}`, 'success')
        await refresh()
      } catch (e) {
        showToast(`Push failed: ${e}`, 'error')
      }
    },
    [selectedShortcut, refresh, showToast],
  )

  const handlePull = useCallback(
    async (passphrase: string) => {
      if (!selectedArtifact) return
      const tag = selectedArtifact.tag
      const repo = selectedArtifact.repo
      setDialog(null)
      setPullProgress({ loaded: 0, total: 0 })
      try {
        const blob = await pullArtifact(repo, tag, passphrase || undefined, (loaded, total) => {
          setPullProgress({ loaded, total })
        })
        setPullProgress(null)
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url
        a.download = `${tag}.tar.gz`
        document.body.appendChild(a)
        a.click()
        document.body.removeChild(a)
        setTimeout(() => URL.revokeObjectURL(url), 1000)
        showToast(`Pulled ${tag}`, 'success')
      } catch (e) {
        setPullProgress(null)
        showToast(`Pull failed: ${e}`, 'error')
      }
    },
    [selectedArtifact, showToast],
  )

  const handleDelete = useCallback(async () => {
    if (!selectedArtifact) return
    setDialog(null)
    try {
      await deleteArtifact(selectedArtifact.repo, selectedArtifact.tag)
      showToast(`Deleted ${selectedArtifact.tag}`, 'success')
      setSelectedArtifact(null)
      await refresh()
    } catch (e) {
      showToast(`Delete failed: ${e}`, 'error')
    }
  }, [selectedArtifact, refresh, setSelectedArtifact, showToast])

  return (
    <div className="app">
      <Sidebar
        shortcuts={shortcuts}
        selected={selectedShortcut}
        onSelect={loadArtifacts}
      />
      <div className="main">
        <Toolbar
          repo={selectedShortcut?.repo ?? ''}
          onPush={() => setDialog('push')}
          onPull={() => setDialog('pull')}
          onDelete={() => setDialog('delete')}
          onRefresh={refresh}
          hasSelection={!!selectedArtifact}
          loading={loading}
        />
        {error && <div className="error-bar">{error}</div>}
        {pullProgress && (
          <div className="progress-bar">
            <div className="progress-text">
              Downloading... {pullProgress.total > 0 ? `${formatBytes(pullProgress.loaded)} / ${formatBytes(pullProgress.total)}` : `${formatBytes(pullProgress.loaded)}`}
            </div>
            <div className="progress-track">
              <div
                className="progress-fill"
                style={{ width: pullProgress.total > 0 ? `${(pullProgress.loaded / pullProgress.total) * 100}%` : '100%' }}
              />
            </div>
          </div>
        )}
        {loading ? (
          <div className="loading">
            <div className="spinner" />
            Loading artifacts...
          </div>
        ) : selectedShortcut ? (
          <ArtifactTable
            artifacts={artifacts}
            selected={selectedArtifact}
            onSelect={setSelectedArtifact}
          />
        ) : (
          <div className="empty-state">Select a shortcut from the sidebar</div>
        )}
        <DetailPanel artifact={selectedArtifact} />
      </div>

      {dialog === 'push' && selectedShortcut && (
        <PushDialog
          repo={selectedShortcut.repo}
          onConfirm={handlePush}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === 'pull' && selectedArtifact && (
        <PullDialog
          artifact={selectedArtifact}
          onConfirm={handlePull}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === 'delete' && selectedArtifact && (
        <DeleteDialog
          artifact={selectedArtifact}
          onConfirm={handleDelete}
          onClose={() => setDialog(null)}
        />
      )}

      {toast && (
        <Toast
          message={toast.message}
          type={toast.type}
          onDone={() => setToast(null)}
        />
      )}
    </div>
  )
}
