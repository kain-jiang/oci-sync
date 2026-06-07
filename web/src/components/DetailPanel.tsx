import type { ArtifactInfo } from '../types'

interface Props {
  artifact: ArtifactInfo | null
}

export function DetailPanel({ artifact }: Props) {
  return (
    <div className="detail-panel">
      <h3>Details</h3>
      {artifact ? (
        <>
          <div className="detail-row">
            <span className="detail-label">Full Name</span>
            <span className="detail-value">{artifact.fullName}</span>
          </div>
          <div className="detail-row">
            <span className="detail-label">Digest</span>
            <span className="detail-value">{artifact.digest}</span>
          </div>
          <div className="detail-row">
            <span className="detail-label">Tag</span>
            <span className="detail-value">{artifact.tag}</span>
          </div>
          <div className="detail-row">
            <span className="detail-label">Encrypted</span>
            <span className="detail-value" style={{ color: artifact.encrypted ? 'var(--yellow)' : 'var(--comment)' }}>
              {artifact.encrypted ? 'Yes' : 'No'}
            </span>
          </div>
          {artifact.labels && Object.keys(artifact.labels).length > 0 && (
            <div className="detail-row">
              <span className="detail-label">Labels</span>
              <span className="detail-value">
                <span className="labels-list">
                  {Object.entries(artifact.labels).map(([k, v]) => (
                    <span key={k} className="label-tag">
                      {k}={v}
                    </span>
                  ))}
                </span>
              </span>
            </div>
          )}
        </>
      ) : (
        <span style={{ color: 'var(--comment)' }}>No artifact selected</span>
      )}
    </div>
  )
}
