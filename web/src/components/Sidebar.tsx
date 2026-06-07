import type { ShortcutInfo } from '../types'

interface Props {
  shortcuts: ShortcutInfo[]
  selected: ShortcutInfo | null
  onSelect: (s: ShortcutInfo) => void
}

export function Sidebar({ shortcuts, selected, onSelect }: Props) {
  return (
    <div className="sidebar">
      <div className="sidebar-title">oci-sync</div>
      <div className="sidebar-list">
        {shortcuts.length === 0 && (
          <div className="sidebar-item" style={{ color: 'var(--comment)' }}>
            No shortcuts configured
          </div>
        )}
        {shortcuts.map((s) => (
          <div
            key={s.name}
            className={`sidebar-item${selected?.name === s.name ? ' active' : ''}`}
            onClick={() => onSelect(s)}
          >
            <span className="name">{s.name}</span>
            <span className="repo">{s.repo}</span>
          </div>
        ))}
      </div>
    </div>
  )
}
