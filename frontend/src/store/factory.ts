function formatPrTitle(identifier: string, rawTitle?: string): string {
    const cleanId = (identifier || 'WAGNER')
      .replace(/^\[|\].*$/g, '')
      .trim()
      .replace(/[^A-Za-z0-9_-]+/g, '-')
      .replace(/^-|-$/g, '') || 'WAGNER'
    let s = (rawTitle || '').split(/\r?\n/)[0]?.trim() || ''
    s = s.replace(/^\[[^\]]+\](?:\([^)]*\))?\s*/, '')
    if (s.toLowerCase().startsWith(cleanId.toLowerCase())) {
      s = s.slice(cleanId.length).trim()
    }
    s = s.replace(/^[—–\-:\s]+/, '').trim()
    if (s.includes(' — ')) {
      s = s.split(' — ')[0].trim()
    }
    s = s.replace(/(?:[\s—–\-:]*#\d+)+\s*$/, '').replace(/[—–\-:\s]+$/, '').trim()
    const lower = s.toLowerCase()
    const isPlaceholder =
      !s ||
      lower.startsWith('factory ') ||
      lower.startsWith('factory-') ||
      lower === cleanId.toLowerCase()
    const subject = isPlaceholder ? 'Automated Factory PR' : Array.from(s).slice(0, 72).join('').trimEnd()
    return `[${cleanId}] ${subject}`
  }