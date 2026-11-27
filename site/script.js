/* lun feature site — no dependencies */
(() => {
  "use strict";

  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

  /* ---------- year, header, reveal ---------- */

  $("#year").textContent = new Date().getFullYear();

  const header = $(".site-header");
  const onScroll = () => header.classList.toggle("scrolled", window.scrollY > 8);
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  const reveals = $$(".reveal");
  if ("IntersectionObserver" in window && !reducedMotion) {
    const io = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          entry.target.classList.add("in");
          io.unobserve(entry.target);
        }
      }
    }, { threshold: 0.1, rootMargin: "0px 0px -40px" });
    reveals.forEach((el) => io.observe(el));
  } else {
    reveals.forEach((el) => el.classList.add("in"));
  }

  /* ---------- copy buttons ---------- */

  $$(".copy").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const text = btn.dataset.copy || "";
      let ok = false;
      try {
        await navigator.clipboard.writeText(text);
        ok = true;
      } catch {
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.setAttribute("readonly", "");
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.select();
        try { ok = document.execCommand("copy"); } catch { ok = false; }
        ta.remove();
      }
      const old = btn.textContent;
      btn.textContent = ok ? "Copied" : "Copy failed";
      btn.classList.toggle("copied", ok);
      setTimeout(() => { btn.textContent = old; btn.classList.remove("copied"); }, 1600);
    });
  });

  /* ---------- toast ---------- */

  const toastEl = $("#toast");
  let toastTimer;
  function toast(html) {
    toastEl.innerHTML = html;
    toastEl.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toastEl.classList.remove("show"), 2600);
  }

  /* ---------- hero terminal ---------- */

  const term = $("#hero-term");
  const heroSeq = [
    {
      cmd: "lun brief --last",
      out: [
        '<span class="k">T-4k2p</span>  review    high  lun  Drag a link onto a card',
        '<span class="k">T-9fdv</span>  review    low   lun  Mobile column snap-scroll',
        '<span class="k">T-8d7a</span>  waiting-on med  lun  Offline PWA cache refresh',
      ],
    },
    {
      cmd: 'lun task T-4k2p --status closed -m "PR #128 merged"',
      out: [
        'Updated <span class="k">T-4k2p</span>: status -> closed',
        '<span class="ok">Committed: PR #128 merged</span>',
      ],
    },
    {
      cmd: "lun log T-4k2p -3 --oneline",
      out: [
        '09:42 coder <span class="upd">UPDATE</span>  PR #128 merged | review -> closed',
        '09:12 coder <span class="com">COMMIT</span>  PR #128 opened for review',
        '09:04 coder <span class="cre">CREATE</span>  open task',
      ],
    },
  ];

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  function renderHeroStatic() {
    term.innerHTML = "";
    for (const step of heroSeq) {
      const line = document.createElement("div");
      line.className = "tline";
      line.innerHTML = '<span class="p">❯</span><span class="cmd"></span>';
      line.querySelector(".cmd").textContent = step.cmd;
      term.appendChild(line);
      for (const html of step.out) {
        const out = document.createElement("div");
        out.className = "tout";
        out.innerHTML = html;
        term.appendChild(out);
      }
    }
  }

  async function runHero() {
    if (reducedMotion) { renderHeroStatic(); return; }
    // Start on the static frame so the first paint is never empty.
    renderHeroStatic();
    await sleep(3200);
    while (true) {
      term.innerHTML = "";
      for (const step of heroSeq) {
        const line = document.createElement("div");
        line.className = "tline";
        line.innerHTML = '<span class="p">❯</span><span class="cmd"></span><span class="caret" aria-hidden="true"></span>';
        term.appendChild(line);
        const cmdEl = line.querySelector(".cmd");
        for (const ch of step.cmd) {
          cmdEl.textContent += ch;
          await sleep(21 + Math.random() * 24);
        }
        line.querySelector(".caret").remove();
        await sleep(260);
        for (const html of step.out) {
          const out = document.createElement("div");
          out.className = "tout";
          out.innerHTML = html;
          term.appendChild(out);
          await sleep(95);
        }
        await sleep(720);
      }
      await sleep(3400);
    }
  }
  if (term) runHero();

  /* ---------- demo board ---------- */

  const STATUS = [
    { id: "planning", label: "Planning" },
    { id: "review", label: "Review" },
    { id: "waiting-on", label: "Waiting-On" },
    { id: "backlog", label: "Backlog" },
    { id: "closed", label: "Closed" },
  ];
  const PROJECT_COLOR = { lun: "#a78bfa", website: "#60a5fa", research: "#fbbf24" };

  const tasks = [
    {
      key: "T-ypee", title: "Drag a link onto a card", status: "review", priority: "high",
      project: "lun", scope: "web", assignee: "coder", link: "PR #128", extraLinks: 1,
      notesHTML: `<h2>Drop targets</h2>
        <ul>
          <li>A URL dragged from any browser tab</li>
          <li>A Mail message saved as <code>.eml</code></li>
          <li>Plain files are refused with a toast</li>
        </ul>
        <p>The drag payload becomes the link label, so <code>message:</code> chips open Mail on macOS and iOS.</p>
        <p><span class="math">E = mc²</span> renders inline, and tables work too.</p>
        <table><thead><tr><th>Source</th><th>Stored as</th></tr></thead>
        <tbody><tr><td>URL</td><td><code>https://…</code></td></tr>
        <tr><td>Mail</td><td><code>message:%3C…%3E</code></td></tr></tbody></table>`,
      history: [
        { time: "09:12", user: "coder", action: "COMMIT", cls: "commit", msg: "PR #128 opened for review" },
        { time: "09:08", user: "coder", action: "LINK", cls: "link", msg: "PR #128" },
        { time: "09:04", user: "web", action: "CREATE", cls: "create", msg: "open task" },
      ],
    },
    {
      key: "T-70ja", title: "Command palette history", status: "planning", priority: "med",
      project: "lun", scope: "tui", assignee: "coder",
      notesHTML: "<p>Palette should remember the last 20 commands, per board. Fuzzy match on the verb, then on task titles.</p>",
      history: [{ time: "08:41", user: "coder", action: "CREATE", cls: "create", msg: "open task" }],
    },
    {
      key: "T-g6q4", title: "Rewrite the landing page", status: "planning", priority: "high",
      project: "website", scope: "design", assignee: "Colton",
      notesHTML: "<p>One page, no build step. Lead with the board, not the architecture.</p>",
      history: [{ time: "08:20", user: "web", action: "CREATE", cls: "create", msg: "from the design review" }],
    },
    {
      key: "T-9fdv", title: "Mobile column snap-scroll", status: "review", priority: "low",
      project: "lun", scope: "web", assignee: "coder", link: "PR #131",
      notesHTML: "<p>Each column is ~85% of the viewport and snaps as you swipe sideways. Test on the narrowest iPhone Safari.</p>",
      history: [
        { time: "09:01", user: "coder", action: "UPDATE", cls: "update", msg: "ready for review" },
        { time: "08:52", user: "coder", action: "LINK", cls: "link", msg: "PR #131" },
      ],
    },
    {
      key: "T-8d7a", title: "Offline PWA cache refresh", status: "waiting-on", priority: "med",
      project: "lun", scope: "web", assignee: "Colton",
      notesHTML: "<p>Cache name bumped to <code>lun-v2</code>. Waiting on a reload check from the phone.</p>",
      history: [{ time: "08:47", user: "coder", action: "COMMIT", cls: "commit", msg: "cache name bumped to lun-v2" }],
    },
    {
      key: "T-btv5", title: "Resubmit cluster jobs", status: "waiting-on", priority: "high",
      project: "research", scope: "physics", assignee: "Colton",
      notesHTML: "<p>Jobs 4812331-2 are in the queue. Check <code>squeue</code> Thursday; resubmit with a longer wall clock if they time out.</p>",
      history: [{ time: "08:30", user: "cli", action: "COMMIT", cls: "commit", msg: "jobs 4812331-2 submitted" }],
    },
    {
      key: "T-4jgz", title: "Attachment thumbnails", status: "backlog", priority: "low",
      project: "lun", scope: "web", assignee: "Colton",
      notesHTML: "<p>Show image thumbnails in the Attachments section instead of a filename row.</p>",
      history: [{ time: "07:58", user: "web", action: "CREATE", cls: "create", msg: "parked for later" }],
    },
    {
      key: "T-711s", title: "Hash-based task keys", status: "closed", priority: "high",
      project: "lun", scope: "core", assignee: "coder",
      notesHTML: "<p>Keys like <code>T-7dqs</code> instead of auto-incrementing numbers, so a board can merge without collisions.</p>",
      history: [
        { time: "07:44", user: "coder", action: "UPDATE", cls: "update", msg: "merged into main" },
        { time: "07:12", user: "coder", action: "COMMIT", cls: "commit", msg: "hash keys shipped in v0.1.0" },
      ],
    },
    {
      key: "T-eedp", title: "Note diffs for every edit", status: "closed", priority: "med",
      project: "lun", scope: "notes", assignee: "coder",
      notesHTML: "<p>Every note write records a unified diff in <code>note_diffs</code>, recoverable with <code>lun log &lt;hash&gt; --old-notes</code>.</p>",
      history: [{ time: "07:35", user: "coder", action: "UPDATE", cls: "update", msg: "closed" }],
    },
  ];

  const board = $("#demo-board");
  let dragKey = null;
  let lastFocused = null;

  function taskCard(t) {
    const color = PROJECT_COLOR[t.project] || "#a78bfa";
    return `<article class="dcard" draggable="true" data-key="${esc(t.key)}" tabindex="0" role="button" aria-label="Open ${esc(t.title)}">
      <div class="dcard-title">${esc(t.title)}</div>
      ${t.link ? `<span class="dlink">🔗 ${esc(t.link)}${t.extraLinks ? ` <b>+${t.extraLinks}</b>` : ""}</span>` : ""}
      <div class="dcard-meta">
        <span class="dkey">${esc(t.key)}</span>
        <span class="dchip ${esc(t.priority)}">${esc(t.priority)}</span>
        <span class="dchip">${esc(t.scope)}</span>
        <span>${esc(t.assignee)}</span>
        <span class="dproj"><i style="background:${color}"></i>${esc(t.project)}</span>
      </div>
    </article>`;
  }

  function renderBoard() {
    board.innerHTML = STATUS.map((s) => {
      const inCol = tasks.filter((t) => t.status === s.id);
      return `<section class="dcol" data-status="${s.id}" aria-label="${s.label}">
        <header class="dcol-head"><span class="dot ${s.id}"></span>${s.label}<span class="count">${inCol.length}</span></header>
        <div class="dcol-body">${inCol.map(taskCard).join("")}</div>
      </section>`;
    }).join("");
  }

  function moveTask(key, status, { focus = false } = {}) {
    const t = tasks.find((x) => x.key === key);
    if (!t || t.status === status || !STATUS.some((s) => s.id === status)) return;
    t.status = status;
    t.history.unshift({
      time: new Date().toTimeString().slice(0, 5),
      user: "web",
      action: "UPDATE",
      cls: "update",
      msg: `status -> ${status}`,
    });
    renderBoard();
    if (focus) $(`.dcard[data-key="${key}"]`, board)?.focus();
    const label = STATUS.find((s) => s.id === status).label;
    toast(`<span class="k">${esc(key)}</span> → ${label} · UPDATE logged`);
    if (!modal.hidden && modalKey === key) fillModal(t);
  }

  if (board) {
    renderBoard();

    board.addEventListener("dragstart", (e) => {
      const card = e.target.closest(".dcard");
      if (!card) return;
      dragKey = card.dataset.key;
      card.classList.add("dragging");
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", dragKey);
    });
    board.addEventListener("dragend", () => {
      dragKey = null;
      $$(".dcard.dragging", board).forEach((c) => c.classList.remove("dragging"));
      $$(".dcol.over", board).forEach((c) => c.classList.remove("over"));
    });
    board.addEventListener("dragover", (e) => {
      const col = e.target.closest(".dcol");
      if (!col) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      col.classList.add("over");
    });
    board.addEventListener("dragleave", (e) => {
      const col = e.target.closest(".dcol");
      if (col && !col.contains(e.relatedTarget)) col.classList.remove("over");
    });
    board.addEventListener("drop", (e) => {
      const col = e.target.closest(".dcol");
      if (!col) return;
      e.preventDefault();
      col.classList.remove("over");
      moveTask(dragKey, col.dataset.status, { focus: true });
    });

    board.addEventListener("click", (e) => {
      const card = e.target.closest(".dcard");
      if (card) openTask(card.dataset.key);
    });
    board.addEventListener("keydown", (e) => {
      const card = e.target.closest(".dcard");
      if (!card) return;
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openTask(card.dataset.key);
      } else if (e.key === "d" || e.key === "u") {
        e.preventDefault();
        const idx = STATUS.findIndex((s) => s.id === tasks.find((t) => t.key === card.dataset.key).status);
        const next = STATUS[Math.max(0, Math.min(STATUS.length - 1, idx + (e.key === "d" ? 1 : -1)))];
        moveTask(card.dataset.key, next.id, { focus: true });
      } else if (e.key === "j" || e.key === "k") {
        e.preventDefault();
        const cards = $$(".dcard", board);
        const i = cards.indexOf(card);
        cards[Math.max(0, Math.min(cards.length - 1, i + (e.key === "j" ? 1 : -1)))]?.focus();
      }
    });
  }

  /* ---------- modal ---------- */

  const modal = $("#modal");
  const modalBody = $(".modal-body", modal);
  let modalKey = null;

  function fillModal(t) {
    $("#modal-key").textContent = t.key;
    $("#modal-title").textContent = t.title;
    $("#modal-fields").innerHTML = `
      <div class="mfield"><label for="modal-status">Status</label>
        <select id="modal-status">${STATUS.map((s) => `<option value="${s.id}"${s.id === t.status ? " selected" : ""}>${s.label}</option>`).join("")}</select>
      </div>
      <div class="mfield"><label>Priority</label><span class="val">${esc(t.priority)}</span></div>
      <div class="mfield"><label>Project</label><span class="val">${esc(t.project)}</span></div>
      <div class="mfield"><label>Scope</label><span class="val">${esc(t.scope)}</span></div>
      <div class="mfield"><label>Assignee</label><span class="val">${esc(t.assignee)}</span></div>
      <div class="mfield"><label>Links</label><span class="val">${t.link ? esc(t.link) + (t.extraLinks ? ` +${t.extraLinks}` : "") : "—"}</span></div>`;
    $("#modal-notes").innerHTML = t.notesHTML || "<p>No notes yet. Try <code>lun task edit … --append-notes</code>.</p>";
    $("#modal-history").innerHTML = t.history.length
      ? t.history.map((h) => `<div class="mhist"><span class="t">${esc(h.time)}</span><span>${esc(h.user)}</span><span class="a ${h.cls}">${h.action}</span><span>${esc(h.msg)}</span></div>`).join("")
      : '<div class="mhist"><span class="t">—</span><span></span><span></span><span>No history yet</span></div>';
    $("#modal-status").addEventListener("change", (e) => moveTask(t.key, e.target.value));
  }

  function openTask(key) {
    const t = tasks.find((x) => x.key === key);
    if (!t) return;
    modalKey = key;
    lastFocused = document.activeElement;
    fillModal(t);
    modal.hidden = false;
    document.body.classList.add("locked");
    modalBody.scrollTop = 0;
    $("#modal-close").focus();
  }

  function closeModal() {
    if (modal.hidden) return;
    modal.hidden = true;
    modalKey = null;
    document.body.classList.remove("locked");
    lastFocused?.focus?.();
  }

  $("#modal-close").addEventListener("click", closeModal);
  $("#modal-done").addEventListener("click", closeModal);
  modal.addEventListener("click", (e) => { if (e.target === modal) closeModal(); });

  document.addEventListener("keydown", (e) => {
    if (!modal.hidden) {
      if (e.key === "Escape") { e.preventDefault(); closeModal(); return; }
      if (e.key === "Tab") {
        const focusable = $$("button, select, [tabindex]:not([tabindex='-1'])", modal)
          .filter((el) => !el.disabled && el.offsetParent !== null);
        if (!focusable.length) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    }
  });

  /* nav active state */
  const navLinks = $$(".nav a[href^='#']");
  const navMap = new Map(navLinks.map((a) => [a.getAttribute("href").slice(1), a]));
  if ("IntersectionObserver" in window) {
    const secIo = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          navLinks.forEach((a) => a.classList.remove("active"));
          navMap.get(entry.target.id)?.classList.add("active");
        }
      }
    }, { rootMargin: "-30% 0px -55% 0px" });
    ["features", "demo", "cli", "tui", "install"].forEach((id) => {
      const el = document.getElementById(id);
      if (el) secIo.observe(el);
    });
  }
})();
