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
      try {
        const blob = await pullArtifact(repo, tag, passphrase || undefined)
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
