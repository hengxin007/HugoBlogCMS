(() => {
  if (window === window.top || window.parent !== window.top) return
  if (location.protocol !== 'http:' || location.hostname !== '127.0.0.1' || !location.port) return
  const parentOrigin = location.ancestorOrigins[0]
  if (!['http://tauri.localhost', 'https://tauri.localhost', 'http://127.0.0.1:1420'].includes(parentOrigin)) return

  const send = (type, details = {}) => {
    window.parent.postMessage({ channel: 'hugo-cms-preview', type, url: location.href, ...details }, parentOrigin)
  }
  const reportLocation = () => send('location')
  for (const event of ['pageshow', 'popstate', 'hashchange']) {
    window.addEventListener(event, reportLocation)
  }
  // Right-clicks in a cross-origin iframe do not reach the application window.
  document.addEventListener('contextmenu', event => {
    if (!event.isTrusted) return
    event.preventDefault()
    send('contextmenu', { x: event.clientX, y: event.clientY })
  }, true)
})()
