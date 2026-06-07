import type { ArtifactInfo } from '../types'

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

interface Props {
  artifacts: ArtifactInfo[]
  selected: ArtifactInfo | null
  onSelect: (a: ArtifactInfo) => void
}

export function ArtifactTable({ artifacts, selected, onSelect }: Props) {
  if (artifacts.length === 0) {
    return (
      <div className="table-container">
        <div className="empty-state">No artifacts found</div>
      </div>
    )
  }

  return (
    <div className="table-container">
      <table className="artifact-table">
        <thead>
          <tr>
            <th>Tag</th>
            <th>Size</th>
            <th>Encrypted</th>
            <th>Version</th>
            <th>Labels</th>
          </tr>
        </thead>
        <tbody>
          {artifacts.map((a) => (
            <tr
              key={a.fullName}
              className={selected?.fullName === a.fullName ? 'selected' : ''}
              onClick={() => onSelect(a)}
            >
              <td className="tag-cell">{a.tag}</td>
              <td>{formatBytes(a.size)}</td>
              <td className={a.encrypted ? 'encrypted-yes' : 'encrypted-no'}>
                {a.encrypted ? 'Yes' : 'No'}
              </td>
              <td>{a.version}</td>
              <td>
                {a.labels && Object.keys(a.labels).length > 0
                  ? Object.entries(a.labels).map(([k, v]) => (
                      <span key={k} className="label-tag">
                        {k}={v}
                      </span>
                    ))
                  : '—'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
