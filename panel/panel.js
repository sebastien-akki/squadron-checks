const me = Math.random().toString(36).slice(2)
const lang = (navigator.language || 'en').toLowerCase()
const fr = lang.startsWith('fr')

const T = fr
  ? {
      title: 'Contrôles',
      rerun: 'Relancer',
      fix: 'Demander à l’agent de corriger',
      running: 'Contrôles en cours…',
      meta: (n, at) => `${n} fichier${n > 1 ? 's' : ''} modifié${n > 1 ? 's' : ''} · ${at}`,
      never: 'Pas encore de contrôle pour cet agent. Ils se lancent à la fin de chacun de ses tours quand il a changé des fichiers, ou tout de suite avec Relancer.',
      neverOff: 'Pas encore de contrôle pour cet agent. Le contrôle automatique est coupé dans les réglages : Relancer les lance.',
      noFiles: 'Aucun fichier modifié dans ce dossier par rapport à la branche principale.',
      nothing: 'Aucun des contrôles ne s’applique aux fichiers modifiés.',
      ok: 'aucun problème',
      problems: (n) => `${n} problème${n > 1 ? 's' : ''}`,
      failed: 'n’a pas pu tourner',
      unused: 'Non utilisés par ce projet : ',
      inserted: 'Ajouté à votre message.',
      hint: 'Chaque contrôle tourne dans Docker, sans réseau, avec la configuration du projet. Le premier lancement télécharge l’image PHP de la version du projet.',
    }
  : {
      title: 'Checks',
      rerun: 'Run again',
      fix: 'Ask the agent to fix',
      running: 'Checking…',
      meta: (n, at) => `${n} changed file${n > 1 ? 's' : ''} · ${at}`,
      never: 'No check for this agent yet. They run at the end of each of its turns when it changed files, or now with Run again.',
      neverOff: 'No check for this agent yet. Automatic checks are off in the settings: Run again runs them.',
      noFiles: 'No changed file in this folder against the main branch.',
      nothing: 'None of the checks applies to the changed files.',
      ok: 'no problem',
      problems: (n) => `${n} problem${n > 1 ? 's' : ''}`,
      failed: 'could not run',
      unused: 'Not used by this project: ',
      inserted: 'Added to your message.',
      hint: 'Each check runs in Docker, without network, with the project’s own config. The first run downloads the PHP image of the project’s version.',
    }

const state = { agent: null, run: null, auto: true, running: false, error: '', note: '' }
const send = (type, extra = {}) => squadron.send({ ...extra, type, to: me, lang })

const relative = new Intl.RelativeTimeFormat(lang, { numeric: 'auto' })
function when(at) {
  const minutes = Math.round((at - Date.now()) / 60000)
  if (minutes > -1) return relative.format(0, 'minute')
  if (minutes > -60) return relative.format(minutes, 'minute')
  if (minutes > -1440) return relative.format(Math.round(minutes / 60), 'hour')
  return new Date(at).toLocaleString(lang)
}

function h(tag, props = {}, ...children) {
  const el = document.createElement(tag)
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null || v === false) continue
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v)
    else if (k in el) el[k] = v
    else el.setAttribute(k, v)
  }
  el.append(...children.flat().filter((c) => c !== null && c !== undefined && c !== false))
  return el
}

function count(run) {
  return run ? run.outcomes.reduce((n, o) => n + o.problems.length, 0) : 0
}

function rerun() {
  state.running = true
  state.error = state.note = ''
  render()
  send('run')
}

function outcome(o) {
  const tone = o.status === 'Ok' ? 'ok' : o.status === 'Problems' ? 'err' : ''
  const text = o.status === 'Ok' ? T.ok : o.status === 'Problems' ? T.problems(o.problems.length) : T.failed
  return h(
    'section',
    { className: 'check' },
    h('div', { className: 'head' }, h('span', { className: `dot ${tone}` }), h('strong', {}, o.label), h('span', { className: 'muted' }, text)),
    o.problems.length > 0 &&
      h(
        'ul',
        { className: 'problems' },
        o.problems.map((p) => h('li', {}, p.file && h('code', {}, p.line ? `${p.file}:${p.line}` : p.file), p.message)),
      ),
    o.note && h('pre', {}, o.note),
  )
}

function body() {
  const run = state.run
  if (state.error) return [h('p', { className: 'error' }, state.error)]
  if (!run) return [h('p', { className: 'muted empty' }, state.auto === false ? T.neverOff : T.never)]
  if (!run.files) return [h('p', { className: 'muted empty' }, T.noFiles)]
  const shown = run.outcomes.filter((o) => o.status !== 'Skipped')
  const unused = run.outcomes.filter((o) => o.status === 'Skipped').map((o) => o.label)
  return [
    !shown.length && h('p', { className: 'muted empty' }, T.nothing),
    shown.map(outcome),
    unused.length > 0 && h('p', { className: 'muted hint' }, T.unused + unused.join(', ')),
  ]
}

function render() {
  const run = state.run
  const status = state.running ? T.running : state.note || (run ? T.meta(run.files, when(run.at)) : '')
  document.getElementById('app').replaceChildren(
    h(
      'header',
      { className: 'bar' },
      h('strong', {}, T.title),
      h('span', { className: 'status muted', role: 'status' }, status),
      h('button', { disabled: state.running || !state.agent, onclick: rerun }, T.rerun),
      count(run) > 0 && h('button', { className: 'primary', disabled: state.running, onclick: () => send('fix') }, T.fix),
    ),
    ...body().flat().filter(Boolean),
    h('p', { className: 'muted hint' }, T.hint),
  )
}

squadron.on((msg) => {
  if (!msg || (msg.to !== me && msg.to !== '*')) return
  if (msg.agent && state.agent && msg.agent !== state.agent) return
  switch (msg.type) {
    case 'init':
      state.agent = msg.agent
      state.run = msg.run
      state.auto = msg.auto
      return render()
    case 'running':
      state.running = true
      return render()
    case 'result':
      state.running = false
      state.run = msg.run
      state.error = ''
      return render()
    case 'done':
      state.running = false
      return render()
    case 'inserted':
      state.note = T.inserted
      render()
      return setTimeout(() => {
        state.note = ''
        render()
      }, 3000)
    case 'error':
      state.running = false
      state.error = msg.message
      return render()
  }
})

render()
send('ready')
