(() => {
  const storageKey = key => `flow-list:${key}`;

  function load(key, defaults = {}) {
    const state = {
      key,
      page: 1,
      pageSize: 20,
      sort: defaults.sort || 'created_at',
      order: defaults.order === 'asc' ? 'asc' : 'desc',
    };
    try {
      const saved = JSON.parse(localStorage.getItem(storageKey(key)) || 'null');
      if (saved && typeof saved === 'object') {
        const size = Number(saved.pageSize);
        if ([10, 20, 50, 100].includes(size)) state.pageSize = size;
        if (typeof saved.sort === 'string' && saved.sort) state.sort = saved.sort;
        if (saved.order === 'asc' || saved.order === 'desc') state.order = saved.order;
      }
    } catch { /* keep defaults */ }
    return state;
  }

  function persist(state) {
    localStorage.setItem(storageKey(state.key), JSON.stringify({
      pageSize: state.pageSize,
      sort: state.sort,
      order: state.order,
    }));
  }

  function rangeText(total, page, pageSize) {
    if (!total) return '共 0 条';
    const start = (page - 1) * pageSize + 1;
    const end = Math.min(total, page * pageSize);
    return `第 ${start}–${end} 条，共 ${total} 条`;
  }

  function paginate(items, state) {
    const pages = Math.max(1, Math.ceil(items.length / state.pageSize));
    state.page = Math.min(Math.max(1, state.page), pages);
    const start = (state.page - 1) * state.pageSize;
    return {pageItems: items.slice(start, start + state.pageSize), pages, total: items.length};
  }

  function render(host, state, total, onChange) {
    if (!host) return;
    const pages = Math.max(1, Math.ceil(total / state.pageSize) || 1);
    host.replaceChildren();
    const bar = document.createElement('div');
    bar.className = 'list-pager';
    const info = document.createElement('span');
    info.textContent = rangeText(total, state.page, state.pageSize);
    const sizes = document.createElement('select');
    sizes.setAttribute('aria-label', '每页条数');
    for (const n of [10, 20, 50, 100]) {
      const option = document.createElement('option');
      option.value = String(n);
      option.textContent = `${n} 条/页`;
      option.selected = n === state.pageSize;
      sizes.append(option);
    }
    sizes.onchange = () => {
      state.pageSize = Number(sizes.value);
      state.page = 1;
      persist(state);
      onChange();
    };
    const nav = document.createElement('div');
    nav.className = 'list-pager-nav';
    const pageLabel = document.createElement('span');
    pageLabel.textContent = `${state.page} / ${pages}`;
    const controls = [
      ['首页', 1, state.page <= 1],
      ['上一页', state.page - 1, state.page <= 1],
      null,
      ['下一页', state.page + 1, state.page >= pages],
      ['末页', pages, state.page >= pages],
    ];
    for (const item of controls) {
      if (!item) { nav.append(pageLabel); continue; }
      const [label, page, disabled] = item;
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'btn';
      button.textContent = label;
      button.disabled = disabled;
      button.onclick = () => { state.page = page; onChange(); };
      nav.append(button);
    }
    bar.append(info, sizes, nav);
    host.append(bar);
  }

  function label(text, field, state) {
    if (state.sort !== field) return text;
    return `${text} ${state.order === 'asc' ? '↑' : '↓'}`;
  }

  function toggle(state, field) {
    if (state.sort === field) state.order = state.order === 'asc' ? 'desc' : 'asc';
    else { state.sort = field; state.order = 'desc'; }
    state.page = 1;
    persist(state);
  }

  window.FlowList = {load, persist, paginate, render, label, toggle};
})();
