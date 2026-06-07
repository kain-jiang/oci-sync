import { useState } from 'react'
import type { ArtifactInfo } from '../types'

interface Props {
  artifact: ArtifactInfo
  onConfirm: (passphrase: string) => void
  onClose: () => void
}

export function PullDialog({ artifact, onConfirm, onClose }: Props) {
  const [passphrase, setPassphrase] = useState('')

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    onConfirm(passphrase)
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Pull Artifact</h2>
        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label>Artifact</label>
            <input type="text" value={artifact.fullName} disabled />
          </div>
          {artifact.encrypted && (
            <div className="form-group">
              <label>Passphrase *</label>
              <input
                type="password"
                value={passphrase}
                onChange={(e) => setPassphrase(e.target.value)}
                placeholder="Required for decryption"
                autoFocus
              />
            </div>
          )}
          <div className="modal-actions">
            <button type="button" className="btn" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn primary">
              Pull &amp; Download
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
