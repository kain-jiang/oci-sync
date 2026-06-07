import { useState, useEffect } from 'react'

interface Props {
  message: string
  type: 'success' | 'error'
  onDone: () => void
}

export function Toast({ message, type, onDone }: Props) {
  const [visible, setVisible] = useState(true)

  useEffect(() => {
    const t = setTimeout(() => {
      setVisible(false)
      setTimeout(onDone, 300)
    }, 3000)
    return () => clearTimeout(t)
  }, [onDone])

  return (
    <div
      className={`toast ${type}`}
      style={{ opacity: visible ? 1 : 0, transition: 'opacity 0.3s' }}
    >
      {message}
    </div>
  )
}
