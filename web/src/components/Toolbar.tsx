interface Props {
  repo: string
  onPush: () => void
  onPull: () => void
  onDelete: () => void
  onRefresh: () => void
  hasSelection: boolean
  loading: boolean
}

export function Toolbar({ repo, onPush, onPull, onDelete, onRefresh, hasSelection, loading }: Props) {
  return (
    <div className="toolbar">
      <span className="title">
        {repo ? `Artifacts — ${repo}` : 'Select a shortcut to begin'}
      </span>
      <button className="btn" onClick={onRefresh} disabled={!repo || loading}>
        Refresh
      </button>
      <button className="btn primary" onClick={onPush} disabled={!repo}>
        Push
      </button>
      <button className="btn" onClick={onPull} disabled={!hasSelection}>
        Pull
      </button>
      <button className="btn danger" onClick={onDelete} disabled={!hasSelection}>
        Delete
      </button>
    </div>
  )
}
