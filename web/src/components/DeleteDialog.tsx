import type { ArtifactInfo } from '../types'

interface Props {
  artifact: ArtifactInfo
  onConfirm: () => void
  onClose: () => void
}

export function DeleteDialog({ artifact, onConfirm, onClose }: Props) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2 style={{ color: 'var(--red)' }}>Delete Artifact</h2>
        <p style={{ marginBottom: 12 }}>
          Are you sure you want to delete this artifact?
        </p>
        <div className="detail-row" style={{ marginBottom: 4 }}>
          <span className="detail-label">Tag</span>
          <span className="detail-value" style={{ color: 'var(--green)' }}>{artifact.tag}</span>
        </div>
        <div className="detail-row" style={{ marginBottom: 4 }}>
          <span className="detail-label">Full Name</span>
          <span className="detail-value" style={{ fontSize: 12 }}>{artifact.fullName}</span>
        </div>
        <div className="detail-row" style={{ marginBottom: 12 }}>
          <span className="detail-label">Digest</span>
          <span className="detail-value" style={{ fontSize: 11, color: 'var(--comment)' }}>{artifact.digest}</span>
        </div>
        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn danger" onClick={onConfirm}>
            Delete
          </button>
        </div>
      </div>
    </div>
  )
}
