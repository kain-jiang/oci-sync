import { useState } from 'react'

interface Props {
  repo: string
  onConfirm: (tag: string, files: FileList, passphrase: string, labels: string) => void
  onClose: () => void
}

export function PushDialog({ repo, onConfirm, onClose }: Props) {
  const [tag, setTag] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const [labels, setLabels] = useState('')
  const [files, setFiles] = useState<FileList | null>(null)

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!tag.trim() || !files?.length) return
    onConfirm(tag.trim(), files, passphrase, labels.trim())
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Push Artifact</h2>
        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label>Repository</label>
            <input type="text" value={repo} disabled />
          </div>
          <div className="form-group">
            <label>Tag *</label>
            <input
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder="e.g. v1.0, latest"
              autoFocus
            />
          </div>
          <div className="form-group">
            <label>Files *</label>
            <div className="file-input-wrapper">
              <input
                type="file"
                multiple
                onChange={(e) => setFiles(e.target.files)}
              />
            </div>
            <div className="hint">Select one or more files to push</div>
          </div>
          <div className="form-group">
            <label>Passphrase (optional)</label>
            <input
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              placeholder="Leave empty for no encryption"
            />
          </div>
          <div className="form-group">
            <label>Labels (optional)</label>
            <input
              type="text"
              value={labels}
              onChange={(e) => setLabels(e.target.value)}
              placeholder="key1=value1, key2=value2"
            />
            <div className="hint">Comma-separated key=value pairs</div>
          </div>
          <div className="modal-actions">
            <button type="button" className="btn" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn primary" disabled={!tag.trim() || !files?.length}>
              Push
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
