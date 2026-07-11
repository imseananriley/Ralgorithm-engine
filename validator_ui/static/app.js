const els = {};
const zoneIds = ["library", "opening", "stack", "battlefield", "hand", "bottomed", "graveyard", "exile", "line"];
const zoneElements = {
  library: "libraryZone",
  opening: "openingZone",
  stack: "stackZone",
  battlefield: "battlefieldZone",
  hand: "handZone",
  bottomed: "bottomedZone",
  graveyard: "graveyardZone",
  exile: "exileZone",
  line: "lineZone",
};

let runs = [];
let manifest = {};
let currentPath = "";
let currentRun = null;
let progressRuns = [];
let currentProgressPath = "";
let progressTimer = null;
let records = [];
let filteredRecords = [];
let selectedRecord = null;
let zones = emptyZones();
let manualLog = [];
let actionIndex = -1;
let playTimer = null;
let dragged = null;
let highlightedCard = "";
let missingImageRequests = new Set();
let imageRequestFailures = new Map();
let imageRequestQueue = [];
let activeImageRequests = 0;
const maxImageRequests = 4;

document.addEventListener("DOMContentLoaded", init);

async function init() {
  cacheElements();
  bindEvents();
  const [manifestPayload, runsPayload] = await Promise.all([
    api("/api/manifest"),
    api("/api/runs"),
  ]);
  manifest = manifestPayload;
  runs = runsPayload.runs || [];
  renderRunSelect();
  await loadProgressRuns();
  progressTimer = setInterval(refreshProgress, 5000);
  if (runs.length) {
    await loadRun(runs[0].path);
  } else {
    els.runSummary.textContent = "No simulator artifacts with validation, cap replay, or game records were found.";
  }
}

function cacheElements() {
  [
    "runSummary",
    "runSelect",
    "progressRunSelect",
    "progressStatus",
    "progressBar",
    "progressComplete",
    "progressCurrent",
    "progressLast",
    "progressEta",
    "progressShards",
    "progressOutputSummary",
    "progressTopRows",
    "fileInput",
    "recordFilter",
    "recordSearch",
    "statGames",
    "statSuccess",
    "statCaps",
    "statSelected",
    "statGemstone",
    "statMull",
    "recordCount",
    "recordList",
    "resetBoardBtn",
    "prevActionBtn",
    "playActionBtn",
    "nextActionBtn",
    "actionScrubber",
    "stateLimitInput",
    "solveBtn",
    "bottomAuditBtn",
    "gambleBtn",
    "recordType",
    "recordTitle",
    "recordDetail",
    "probeResult",
    "bottomAuditResult",
    "gambleResult",
    "spotlightCard",
    "actionCount",
    "actionList",
    "manualLog",
    "clearManualBtn",
    "saveNoteBtn",
    "noteText",
    "libraryCount",
    "openingCount",
    "stackCount",
    "battlefieldCount",
    "handCount",
    "bottomedCount",
    "graveyardCount",
    "exileCount",
    "lineCount",
    "mulliganCount",
    "mulliganList",
  ].forEach((id) => {
    els[id] = document.getElementById(id);
  });
  for (const [zone, id] of Object.entries(zoneElements)) {
    els[id] = document.getElementById(id);
  }
}

function bindEvents() {
  els.runSelect.addEventListener("change", () => loadRun(els.runSelect.value));
  els.progressRunSelect.addEventListener("change", () => {
    currentProgressPath = els.progressRunSelect.value;
    refreshProgress();
  });
  els.recordFilter.addEventListener("change", renderRecords);
  els.recordSearch.addEventListener("input", renderRecords);
  els.fileInput.addEventListener("change", loadLocalFile);
  els.resetBoardBtn.addEventListener("click", () => selectedRecord && selectRecord(selectedRecord.key));
  els.prevActionBtn.addEventListener("click", () => stepAction(-1));
  els.nextActionBtn.addEventListener("click", () => stepAction(1));
  els.playActionBtn.addEventListener("click", togglePlayback);
  els.actionScrubber.addEventListener("input", () => setActionIndex(Number(els.actionScrubber.value)));
  els.solveBtn.addEventListener("click", runRustProbe);
  els.bottomAuditBtn.addEventListener("click", runBottomAudit);
  els.gambleBtn.addEventListener("click", runGambleAudit);
  els.clearManualBtn.addEventListener("click", () => {
    manualLog = [];
    renderManualLog();
  });
  els.saveNoteBtn.addEventListener("click", saveNote);
  document.addEventListener("dragover", (event) => event.preventDefault());
  for (const zone of zoneIds) {
    const container = document.querySelector(`[data-zone="${zone}"]`);
    container.addEventListener("dragover", (event) => {
      event.preventDefault();
      container.classList.add("drag-over");
    });
    container.addEventListener("dragleave", () => container.classList.remove("drag-over"));
    container.addEventListener("drop", (event) => {
      event.preventDefault();
      container.classList.remove("drag-over");
      dropCard(zone);
    });
  }
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    cache: "no-store",
    headers: options.body ? { "Content-Type": "application/json" } : undefined,
    ...options,
  });
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || `${path} returned ${response.status}`);
  }
  return payload;
}

function renderRunSelect() {
  els.runSelect.innerHTML = "";
  for (const run of runs) {
    const option = document.createElement("option");
    option.value = run.path;
    option.textContent = `${run.label} (${run.counts.validation_records || 0} validation, ${run.counts.cap_replay_records || 0} caps, ${run.counts.game_records || 0} games)`;
    els.runSelect.append(option);
  }
}

async function loadProgressRuns() {
  try {
    const payload = await api("/api/progress-runs");
    progressRuns = payload.runs || [];
    renderProgressRunSelect();
    if (progressRuns.length) {
      currentProgressPath = progressRuns[0].path;
      await refreshProgress();
    }
  } catch (error) {
    els.progressStatus.textContent = error.message;
  }
}

function renderProgressRunSelect() {
  els.progressRunSelect.innerHTML = "";
  if (!progressRuns.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "No progress runs";
    els.progressRunSelect.append(option);
    return;
  }
  for (const run of progressRuns) {
    const option = document.createElement("option");
    option.value = run.path;
    option.textContent = `${run.label} (${run.completed_variants}/${run.total_variants})`;
    els.progressRunSelect.append(option);
  }
}

async function refreshProgress() {
  if (!currentProgressPath) return;
  try {
    const payload = await api(`/api/progress?path=${encodeURIComponent(currentProgressPath)}`);
    renderProgress(payload.payload || {});
  } catch (error) {
    els.progressStatus.textContent = error.message;
  }
}

function renderProgress(progress) {
  const total = Number(progress.total_variants || 0);
  const complete = Number(progress.completed_variants || 0);
  const pct = total ? complete / total : 0;
  const status = progress.run_status || "idle";
  const current = status === "paused" && progress.current_variant
    ? `paused: ${progress.current_variant}`
    : progress.current_variant || status;
  const latest = progress.latest_completed?.name || "none";
  els.progressStatus.textContent = `${formatPct(pct)} complete | ${status} | ${progress.out_dir || ""}`;
  els.progressBar.style.width = `${Math.max(0, Math.min(100, pct * 100)).toFixed(2)}%`;
  els.progressComplete.textContent = `${complete} / ${total}`;
  els.progressCurrent.textContent = current;
  els.progressLast.textContent = latest;
  els.progressEta.textContent = progress.eta_text || "unknown";
  els.progressShards.textContent = formatShardSummary(progress.running_shards || []);
  const outputs = progress.outputs || {};
  const progressRows = progress.intermediate_rows || progress.top_intermediate_rows || [];
  els.progressOutputSummary.textContent = [
    outputs.intermediate_summary_csv ? `CSV: ${outputs.intermediate_summary_csv}` : "",
    outputs.progress_md ? `Report: ${outputs.progress_md}` : "",
    progress.intermediate_row_count != null ? `Rows: ${progress.intermediate_row_count}` : "",
  ].filter(Boolean).join(" | ");
  renderProgressRows(progressRows);
}

function formatShardSummary(shards) {
  if (!shards.length) return "0";
  const games = shards.reduce((total, shard) => total + Number(shard.games || 0), 0);
  const successes = shards.reduce((total, shard) => total + Number(shard.successes || 0), 0);
  return `${shards.length} (${successes}/${games})`;
}

function renderProgressRows(rows) {
  els.progressTopRows.innerHTML = "";
  for (const row of rows) {
    const tr = document.createElement("tr");
    const ciLow = Number(row.score_delta_ci_low);
    const ciHigh = Number(row.score_delta_ci_high);
    const scoreDelta = row.score_delta ?? row.score_delta_mean;
    tr.innerHTML = `
      <td>${escapeHtml(row.variant || "")}</td>
      <td>${escapeHtml(`${row.cut || ""} -> ${row.add || ""}`)}</td>
      <td class="${deltaClass(row.rate_delta)}">${formatSignedPct(row.rate_delta)}</td>
      <td class="${deltaClass(scoreDelta)}">${formatSignedNumber(scoreDelta)}</td>
      <td>${Number.isFinite(ciLow) && Number.isFinite(ciHigh) ? `${formatSignedNumber(ciLow)} to ${formatSignedNumber(ciHigh)}` : "-"}</td>
    `;
    els.progressTopRows.append(tr);
  }
  if (!rows.length) {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td colspan="5">Intermediate paired rows will appear after baseline and one candidate finish.</td>`;
    els.progressTopRows.append(tr);
  }
}

async function loadRun(path) {
  pausePlayback();
  currentPath = path;
  els.runSelect.value = path;
  const payload = await api(`/api/run?path=${encodeURIComponent(path)}`);
  setCurrentRun(payload.payload, payload.path);
}

function setCurrentRun(payload, pathLabel) {
  currentRun = payload;
  currentPath = pathLabel || currentPath || "local file";
  records = normalizeRecords(payload);
  selectedRecord = null;
  els.runSummary.textContent = summarizeRun(payload, currentPath);
  renderStats();
  renderRecords();
  if (filteredRecords.length) {
    selectRecord(filteredRecords[0].key);
  } else {
    zones = emptyZones();
    renderBoard();
  }
}

function summarizeRun(payload, pathLabel) {
  const ev = payload.evaluation || {};
  const pieces = [
    pathLabel,
    `${ev.games || payload.eval_games || 0} games`,
    `${formatPct(ev.success_rate)} success`,
    `${ev.cap_misses || 0} caps`,
  ];
  return pieces.join(" | ");
}

function normalizeRecords(payload) {
  const ev = payload.evaluation || {};
  const out = [];
  for (const [index, row] of (ev.validation_records || []).entries()) {
    out.push({ ...row, sourceType: "validation", key: `validation:${row.game_index}:${index}` });
  }
  for (const [index, row] of (ev.cap_replay_records || []).entries()) {
    out.push({ ...row, sourceType: "cap", hit: false, capped: true, key: `cap:${row.game_index}:${index}` });
  }
  for (const [index, row] of (ev.game_records || []).entries()) {
    out.push({ ...row, sourceType: "game", key: `game:${row.game_index}:${index}` });
  }
  out.sort((a, b) => Number(a.game_index || 0) - Number(b.game_index || 0) || sourceRank(a.sourceType) - sourceRank(b.sourceType));
  return out;
}

function sourceRank(type) {
  return { validation: 0, cap: 1, game: 2 }[type] ?? 9;
}

function renderStats() {
  const ev = currentRun?.evaluation || {};
  els.statGames.textContent = String(ev.games || currentRun?.eval_games || 0);
  els.statSuccess.textContent = formatPct(ev.success_rate);
  els.statCaps.textContent = String(ev.cap_misses || 0);
  if (selectedRecord) {
    els.statSelected.textContent = `#${selectedRecord.game_index}`;
    els.statGemstone.textContent = selectedRecord.gemstone_caverns_live ? "live" : "dead";
    els.statMull.textContent = `${selectedRecord.stage ?? "-"} / ${selectedRecord.bottom_count ?? "-"}`;
  } else {
    els.statSelected.textContent = "none";
    els.statGemstone.textContent = "dead";
    els.statMull.textContent = "0 / 0";
  }
}

function renderRecords() {
  const filter = els.recordFilter.value;
  const search = els.recordSearch.value.trim().toLowerCase();
  filteredRecords = records.filter((record) => recordMatches(record, filter, search));
  els.recordCount.textContent = String(filteredRecords.length);
  els.recordList.innerHTML = "";
  for (const record of filteredRecords) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `record-button ${selectedRecord?.key === record.key ? "active" : ""}`;
    button.addEventListener("click", () => selectRecord(record.key));
    const main = document.createElement("span");
    main.className = "record-main";
    const title = document.createElement("span");
    title.className = "record-title";
    title.textContent = `Game ${record.game_index} | ${record.sourceType}`;
    const sub = document.createElement("span");
    sub.className = "record-sub";
    sub.textContent = recordSubtitle(record);
    main.append(title, sub);
    const badge = document.createElement("span");
    badge.className = `badge ${badgeClass(record)}`;
    badge.textContent = badgeText(record);
    button.append(main, badge);
    els.recordList.append(button);
  }
}

function recordMatches(record, filter, search) {
  if (filter === "miss" && (record.hit || record.sourceType === "game" && record.hit)) return false;
  if (filter === "capped" && !record.capped) return false;
  if (filter === "hit" && !record.hit) return false;
  if (filter === "traced" && !(record.line_actions || []).length) return false;
  if (!search) return true;
  const haystack = [
    record.game_index,
    record.sourceType,
    ...(record.visible_hand || []),
    ...(record.bottomed || []),
    ...(record.keep || []),
    ...(record.library || []).slice(0, 12),
    ...(record.line_cards || []),
    ...(record.line_actions || []),
    ...decisionSearchTerms(record),
  ].join(" ").toLowerCase();
  return haystack.includes(search);
}

function recordSubtitle(record) {
  const parts = [
    `stage ${record.stage ?? "-"}`,
    `bottom ${record.bottom_count ?? "-"}`,
    record.gemstone_caverns_live ? "Gem live" : "Gem dead",
  ];
  if ((record.visible_hand || []).length) parts.push(`pre ${record.visible_hand.length}`);
  if ((record.bottomed || []).length) parts.push(`bottomed ${(record.bottomed || []).join(", ")}`);
  if (record.turn) parts.push(`turn ${record.turn}`);
  if (record.capped) parts.push("capped");
  if ((record.line_actions || []).length) parts.push(`${record.line_actions.length} actions`);
  return parts.join(" | ");
}

function badgeClass(record) {
  if (record.capped && !record.hit) return "capped";
  if (record.hit) return "hit";
  if ((record.line_actions || []).length) return "trace";
  return "miss";
}

function badgeText(record) {
  if (record.capped && !record.hit) return "CAP";
  if (record.hit) return "HIT";
  if ((record.line_actions || []).length) return "TRACE";
  return "MISS";
}

function selectRecord(key) {
  pausePlayback();
  selectedRecord = records.find((record) => record.key === key) || null;
  actionIndex = -1;
  highlightedCard = "";
  manualLog = [];
  els.probeResult.textContent = "";
  els.bottomAuditResult.textContent = "";
  els.gambleResult.textContent = "";
  zones = zonesFromRecord(selectedRecord);
  renderStats();
  renderRecords();
  renderBoard();
  renderInspector();
  renderActions();
  renderManualLog();
}

function zonesFromRecord(record) {
  const next = emptyZones();
  if (!record) return next;
  const visibleHand = visibleHandForRecord(record);
  const bottomed = record.bottomed || [];
  const keep = record.keep || removeCardsByName(visibleHand, bottomed);
  next.opening = asCards(visibleHand);
  next.bottomed = asCards(bottomed);
  next.hand = asCards(keep);
  next.library = asCards(record.library || []);
  next.line = asCards(record.line_cards || []);
  if (!next.hand.length && !next.line.length && (record.line_actions || []).length) {
    next.line = asCards(uniqueActionCards(record));
  }
  return next;
}

function visibleHandForRecord(record) {
  if ((record.visible_hand || []).length) return record.visible_hand;
  if ((record.keep || []).length || (record.bottomed || []).length) {
    return [...(record.keep || []), ...(record.bottomed || [])];
  }
  const lastDecision = (record.mulligan_decisions || []).at(-1);
  return lastDecision?.visible_hand || [];
}

function removeCardsByName(cards, removed) {
  const counts = new Map();
  for (const name of removed || []) counts.set(name, (counts.get(name) || 0) + 1);
  const out = [];
  for (const name of cards || []) {
    const count = counts.get(name) || 0;
    if (count > 0) {
      counts.set(name, count - 1);
    } else {
      out.push(name);
    }
  }
  return out;
}

function countNames(names) {
  const counts = new Map();
  for (const name of names || []) counts.set(name, (counts.get(name) || 0) + 1);
  return counts;
}

function sameCardMultiset(left, right) {
  const leftCounts = countNames(left);
  const rightCounts = countNames(right);
  if (leftCounts.size !== rightCounts.size) return false;
  for (const [name, count] of leftCounts.entries()) {
    if ((rightCounts.get(name) || 0) !== count) return false;
  }
  return true;
}

function isSubCardMultiset(subset, fullSet) {
  const subsetCounts = countNames(subset);
  const fullCounts = countNames(fullSet);
  for (const [name, count] of subsetCounts.entries()) {
    if ((fullCounts.get(name) || 0) < count) return false;
  }
  return true;
}

function zoneNames(zone) {
  return (zones[zone] || []).map((card) => card.name);
}

function currentProbeLibrary() {
  if (!selectedRecord) return zoneNames("library");
  const opening = zoneNames("opening");
  const hand = zoneNames("hand");
  const recordVisible = visibleHandForRecord(selectedRecord);
  const recordLibrary = selectedRecord.library || [];
  const originalBottomCount = (selectedRecord.bottomed || []).length;
  const canRebuildBottoms =
    opening.length > 0 &&
    recordVisible.length > 0 &&
    originalBottomCount > 0 &&
    recordLibrary.length >= originalBottomCount &&
    sameCardMultiset(opening, recordVisible) &&
    isSubCardMultiset(hand, opening);
  if (!canRebuildBottoms) return zoneNames("library");
  const baseLibrary = recordLibrary.slice(0, recordLibrary.length - originalBottomCount);
  return baseLibrary.concat(removeCardsByName(opening, hand));
}

function asCards(names) {
  return (names || []).map((name, index) => ({
    id: `${String(name)}:${index}:${Math.random().toString(36).slice(2)}`,
    name: String(name),
    tapped: false,
  }));
}

function uniqueActionCards(record) {
  const names = [];
  for (const action of record.line_actions || []) {
    const name = findCardNameInText(action);
    if (name && !names.includes(name)) names.push(name);
  }
  return names;
}

function emptyZones() {
  return { library: [], opening: [], stack: [], battlefield: [], hand: [], bottomed: [], graveyard: [], exile: [], line: [] };
}

function renderBoard() {
  for (const zone of zoneIds) {
    const el = els[zoneElements[zone]];
    el.innerHTML = "";
    const cards = zones[zone] || [];
    for (const [index, card] of cards.entries()) {
      el.append(renderCard(card, zone, index));
    }
    const countEl = els[`${zone}Count`];
    if (countEl) countEl.textContent = String(cards.length);
  }
}

function renderCard(card, zone, index) {
  const button = document.createElement("div");
  button.className = `card ${card.tapped ? "tapped" : ""} ${card.name === highlightedCard ? "highlight" : ""}`;
  button.draggable = true;
  button.dataset.zone = zone;
  button.dataset.index = String(index);
  button.dataset.name = card.name;
  button.title = card.name;
  const image = imageForCard(card.name);
  if (image) {
    const img = document.createElement("img");
    img.src = image;
    img.alt = card.name;
    img.loading = "lazy";
    img.onerror = () => {
      button.innerHTML = "";
      button.append(fallbackCard(card.name));
    };
    button.append(img);
  } else {
    if (shouldAutoLoadImage(zone, index)) requestCardImage(card.name);
    button.append(fallbackCard(card.name));
  }
  button.addEventListener("dragstart", () => {
    dragged = { zone, index };
  });
  button.addEventListener("click", () => {
    highlightedCard = card.name;
    renderSpotlight(card.name);
    renderBoard();
  });
  button.addEventListener("dblclick", () => {
    card.tapped = !card.tapped;
    manualLog.push(`${card.tapped ? "tap" : "untap"} ${card.name}`);
    renderBoard();
    renderManualLog();
  });
  return button;
}

function fallbackCard(name) {
  const div = document.createElement("div");
  div.className = "card-fallback";
  div.textContent = name;
  return div;
}

function imageForCard(name) {
  const exact = manifest[name];
  if (exact?.image) return exact.image;
  if (name.includes(" // ")) {
    const front = name.split(" // ")[0];
    if (manifest[front]?.image) return manifest[front].image;
  }
  const match = Object.values(manifest).find((card) => card.name === name || card.back_name === name);
  return match?.image || "";
}

function shouldAutoLoadImage(zone, index) {
  return zone !== "library" || index < 8;
}

function requestCardImage(name) {
  const failedAt = imageRequestFailures.get(name) || 0;
  if (!name || missingImageRequests.has(name) || Date.now() - failedAt < 60000 || imageForCard(name)) return;
  missingImageRequests.add(name);
  imageRequestQueue.push(name);
  drainImageRequestQueue();
}

function drainImageRequestQueue() {
  while (activeImageRequests < maxImageRequests && imageRequestQueue.length) {
    const name = imageRequestQueue.shift();
    activeImageRequests += 1;
    fetchCardImage(name).finally(() => {
      activeImageRequests -= 1;
      drainImageRequestQueue();
    });
  }
}

async function fetchCardImage(name) {
  try {
    const payload = await api(`/api/card?name=${encodeURIComponent(name)}`);
    manifest[name] = { ...(manifest[name] || {}), ...payload };
    if (payload.name && payload.name !== name) {
      manifest[payload.name] = { ...(manifest[payload.name] || {}), ...payload };
    }
    imageRequestFailures.delete(name);
    renderBoard();
    if (highlightedCard === name || els.spotlightCard.dataset.name === name) renderSpotlight(name);
  } catch (error) {
    imageRequestFailures.set(name, Date.now());
  } finally {
    missingImageRequests.delete(name);
  }
}

function dropCard(targetZone) {
  if (!dragged) return;
  const source = zones[dragged.zone];
  const [card] = source.splice(dragged.index, 1);
  if (!card) return;
  zones[targetZone].push(card);
  manualLog.push(`${card.name}: ${dragged.zone} -> ${targetZone}`);
  dragged = null;
  renderBoard();
  renderManualLog();
}

function renderInspector() {
  const record = selectedRecord;
  if (!record) {
    els.recordTitle.textContent = "No record selected";
    els.recordDetail.textContent = "";
    els.recordType.textContent = "Record";
    els.spotlightCard.innerHTML = "";
    renderMulligans();
    return;
  }
  els.recordType.textContent = record.sourceType;
  els.recordTitle.textContent = `Game ${record.game_index}`;
  els.recordDetail.textContent = recordDetail(record);
  const first = (record.keep || record.line_cards || [])[0];
  renderSpotlight(first || "");
  renderMulligans();
}

function recordDetail(record) {
  const status = record.hit ? `hit on turn ${record.turn}` : record.capped ? "capped miss" : "miss";
  const bottomed = (record.bottomed || []).length ? ` | bottomed ${(record.bottomed || []).join(", ")}` : "";
  return `${status} | stage ${record.stage ?? "-"} | bottom ${record.bottom_count ?? "-"} | ${record.gemstone_caverns_live ? "Gemstone live" : "Gemstone dead"}${bottomed}`;
}

function renderSpotlight(name) {
  els.spotlightCard.innerHTML = "";
  els.spotlightCard.dataset.name = name || "";
  if (!name) return;
  const image = imageForCard(name);
  if (image) {
    const img = document.createElement("img");
    img.src = image;
    img.alt = name;
    els.spotlightCard.append(img);
  } else {
    requestCardImage(name);
    els.spotlightCard.append(fallbackCard(name));
  }
}

function renderMulligans() {
  const decisions = selectedRecord?.mulligan_decisions || [];
  els.mulliganCount.textContent = String(decisions.length);
  els.mulliganList.innerHTML = "";
  for (const [index, decision] of decisions.entries()) {
    const li = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = decision.keep ? "keep" : "mull";
    const verdict = decision.keep ? "KEEP" : "MULL";
    const bottom = (decision.best_bottom || []).join(", ") || "none";
    button.innerHTML = `
      <span>${index + 1}. stage ${decision.stage} | bottom ${decision.bottom_count} | ${verdict}</span>
      <small>EV ${formatNumber(decision.score_ev)} / threshold ${formatNumber(decision.keep_threshold)} | bottom: ${escapeHtml(bottom)}</small>
    `;
    button.addEventListener("click", () => showMulliganDecision(decision));
    li.append(button);
    els.mulliganList.append(li);
  }
}

function showMulliganDecision(decision) {
  const visible = decision.visible_hand || [];
  const bottom = decision.best_bottom || [];
  zones.opening = asCards(visible);
  zones.bottomed = asCards(bottom);
  zones.hand = asCards(removeCardsByName(visible, bottom));
  highlightedCard = "";
  manualLog.push(`inspect mulligan stage ${decision.stage}: ${decision.keep ? "keep" : "mull"}`);
  renderBoard();
  renderManualLog();
  renderSpotlight(visible[0] || "");
}

function renderActions() {
  const actions = selectedRecord?.line_actions || [];
  els.actionCount.textContent = String(actions.length);
  els.actionScrubber.max = String(Math.max(0, actions.length - 1));
  els.actionScrubber.value = String(Math.max(0, actionIndex));
  els.actionList.innerHTML = "";
  for (const [index, action] of actions.entries()) {
    const li = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = index === actionIndex ? "active" : "";
    button.textContent = `${index + 1}. ${action}`;
    button.addEventListener("click", () => setActionIndex(index));
    li.append(button);
    els.actionList.append(li);
  }
}

function stepAction(delta) {
  const actions = selectedRecord?.line_actions || [];
  if (!actions.length) return;
  setActionIndex(Math.max(0, Math.min(actions.length - 1, actionIndex + delta)));
}

function setActionIndex(index) {
  const actions = selectedRecord?.line_actions || [];
  if (!actions.length) return;
  actionIndex = Math.max(0, Math.min(actions.length - 1, index));
  const action = actions[actionIndex];
  highlightedCard = findCardNameInText(action) || "";
  applyActionHint(action, highlightedCard);
  renderBoard();
  renderActions();
  if (highlightedCard) renderSpotlight(highlightedCard);
}

function togglePlayback() {
  if (playTimer) {
    pausePlayback();
    return;
  }
  els.playActionBtn.textContent = "Pause";
  playTimer = setInterval(() => {
    const actions = selectedRecord?.line_actions || [];
    if (!actions.length || actionIndex >= actions.length - 1) {
      pausePlayback();
      return;
    }
    stepAction(1);
  }, 650);
}

function pausePlayback() {
  if (playTimer) clearInterval(playTimer);
  playTimer = null;
  if (els.playActionBtn) els.playActionBtn.textContent = "Play";
}

function applyActionHint(action, cardName) {
  if (!cardName) return;
  const lower = action.toLowerCase();
  if (lower.startsWith("tap ")) {
    const card = findCardInZones(cardName);
    if (card) card.tapped = true;
    return;
  }
  if (lower.startsWith("play ")) {
    moveNamedCard(cardName, ["hand", "line"], "battlefield");
    return;
  }
  if (lower.startsWith("cast ")) {
    const typeLine = manifest[cardName]?.type_line || "";
    const permanent = /Artifact|Creature|Enchantment|Planeswalker|Battle|Land/.test(typeLine);
    moveNamedCard(cardName, ["hand", "line"], permanent ? "battlefield" : "graveyard");
    return;
  }
  if (lower.startsWith("sac ") || lower.startsWith("discard ")) {
    moveNamedCard(cardName, zoneIds, "graveyard");
    return;
  }
  if (lower.startsWith("exile ")) {
    moveNamedCard(cardName, zoneIds, "exile");
  }
}

function findCardInZones(name) {
  for (const zone of zoneIds) {
    const card = zones[zone].find((item) => item.name === name);
    if (card) return card;
  }
  return null;
}

function moveNamedCard(name, fromZones, targetZone) {
  for (const zone of fromZones) {
    const index = zones[zone].findIndex((item) => item.name === name);
    if (index >= 0) {
      const [card] = zones[zone].splice(index, 1);
      zones[targetZone].push(card);
      return true;
    }
  }
  return false;
}

function findCardNameInText(text) {
  const names = candidateCardNames();
  return names.find((name) => text.includes(name)) || "";
}

function candidateCardNames() {
  const names = new Set(Object.keys(manifest));
  if (currentRun?.deck && Array.isArray(currentRun.deck)) {
    for (const name of currentRun.deck) names.add(name);
  }
  for (const record of records) {
    for (const name of record.visible_hand || []) names.add(name);
    for (const name of record.bottomed || []) names.add(name);
    for (const name of record.keep || []) names.add(name);
    for (const name of record.line_cards || []) names.add(name);
    for (const decision of record.mulligan_decisions || []) {
      for (const name of decision.visible_hand || []) names.add(name);
      for (const name of decision.best_bottom || []) names.add(name);
    }
  }
  return [...names].sort((a, b) => b.length - a.length);
}

function decisionSearchTerms(record) {
  const out = [];
  for (const decision of record.mulligan_decisions || []) {
    out.push(...(decision.visible_hand || []), ...(decision.best_bottom || []));
  }
  return out;
}

async function runRustProbe() {
  if (!selectedRecord) return;
  els.probeResult.textContent = "running...";
  try {
    const payload = {
      record: selectedRecord,
      hand: zoneNames("hand"),
      library: currentProbeLibrary(),
      state_limit: Number(els.stateLimitInput.value || 400000),
    };
    const result = await api("/api/solve", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    els.probeResult.textContent = `${result.hit ? "hit" : "miss"} | turn ${result.turn ?? "-"} | capped ${Boolean(result.capped)} | ${result.label || ""}`;
  } catch (error) {
    els.probeResult.textContent = error.message;
  }
}

async function runBottomAudit() {
  if (!selectedRecord) return;
  els.bottomAuditResult.textContent = "auditing bottom choices...";
  try {
    const visibleHand = zoneNames("opening");
    const bottomed = zoneNames("bottomed");
    const payload = {
      record: selectedRecord,
      visible_hand: visibleHand.length ? visibleHand : [...zoneNames("hand"), ...bottomed],
      bottom_count: bottomed.length || selectedRecord.bottom_count || 0,
      state_limit: Number(els.stateLimitInput.value || 400000),
    };
    const result = await api("/api/bottom-audit", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    els.bottomAuditResult.textContent = formatBottomAudit(result);
  } catch (error) {
    els.bottomAuditResult.textContent = error.message;
  }
}

async function runGambleAudit() {
  if (!selectedRecord) return;
  els.gambleResult.textContent = "auditing Gamble...";
  try {
    const payload = {
      record: selectedRecord,
      hand: zoneNames("hand"),
      library: currentProbeLibrary(),
      state_limit: Number(els.stateLimitInput.value || 400000),
      sample_count: 64,
    };
    const result = await api("/api/gamble-audit", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    els.gambleResult.textContent = formatGambleAudit(result);
  } catch (error) {
    els.gambleResult.textContent = error.message;
  }
}

function formatSolveSummary(result) {
  const status = result.hit ? "hit" : "miss";
  const turn = result.turn ?? "-";
  const cap = result.capped ? " capped" : "";
  const label = result.label ? ` | ${result.label}` : "";
  return `${status} t${turn}${cap}${label}`;
}

function solveDiffers(left, right) {
  if (!right) return false;
  return Boolean(left?.hit) !== Boolean(right.hit) ||
    (left?.turn ?? null) !== (right.turn ?? null) ||
    Boolean(left?.capped) !== Boolean(right.capped) ||
    (left?.label || "") !== (right.label || "");
}

function formatStrictDiff(row) {
  const parts = [];
  if (solveDiffers(row.current, row.strict_current)) {
    parts.push(`current strict ${formatSolveSummary(row.strict_current || {})}`);
  }
  if (solveDiffers(row.optimistic, row.strict_optimistic)) {
    parts.push(`optimistic strict ${formatSolveSummary(row.strict_optimistic || {})}`);
  }
  if (solveDiffers(row.no_gamble, row.strict_no_gamble)) {
    parts.push(`no Gamble strict ${formatSolveSummary(row.strict_no_gamble || {})}`);
  }
  return parts.join("; ");
}

function formatSeedExamples(label, examples) {
  if (!examples || !examples.length) return `${label}: none`;
  return `${label}: ${examples.map((item) => `${item.gamble_seed}:${formatSolveSummary(item)}`).join("; ")}`;
}

function formatGambleAudit(result) {
  const stochastic = result.stochastic || {};
  const samples = stochastic.samples || 0;
  const hits = stochastic.hits || 0;
  const hitRate = samples ? `${(100 * hits / samples).toFixed(1)}%` : "n/a";
  const lines = [];
  if (!result.has_gamble) {
    lines.push("Gamble is not in the kept hand. Move it into Kept Hand and move another visible card to Bottomed, then audit again.");
  }
  lines.push(`current seed: ${formatSolveSummary(result.current || {})}`);
  lines.push(`Gamble off: ${formatSolveSummary(result.no_gamble || {})}`);
  lines.push(`optimistic Gamble: ${formatSolveSummary(result.optimistic || {})}`);
  lines.push(`stochastic sweep: ${hits}/${samples} (${hitRate})`);
  lines.push(formatSeedExamples("hit seeds", stochastic.hit_examples));
  lines.push(formatSeedExamples("miss seeds", stochastic.miss_examples));
  return lines.join("\n");
}

function formatBottomAudit(result) {
  const rows = result.rows || [];
  const lines = [
    `${result.hit_count || 0}/${result.candidate_count || 0} bottom choices hit at cap ${result.state_limit || "-"} | shuffle-sensitive ${result.shuffle_sensitive_count || 0}`,
  ];
  const shown = rows.slice(0, 40);
  for (const [index, row] of shown.entries()) {
    const marker = `${row.recorded ? " [recorded]" : ""}${row.shuffle_sensitive ? " [shuffle-sensitive]" : ""}`;
    lines.push(`${index + 1}. bottom${marker}: ${(row.bottom || []).join(", ") || "none"}`);
    lines.push(`   keep: ${(row.keep || []).join(", ") || "none"}`);
    lines.push(`   current: ${formatSolveSummary(row.current || {})}; optimistic: ${formatSolveSummary(row.optimistic || {})}; no Gamble: ${formatSolveSummary(row.no_gamble || {})}`);
    const strictDiff = formatStrictDiff(row);
    if (strictDiff) lines.push(`   ${strictDiff}`);
  }
  if (rows.length > shown.length) lines.push(`... ${rows.length - shown.length} more choices omitted`);
  return lines.join("\n");
}

async function saveNote() {
  if (!selectedRecord) return;
  const note = els.noteText.value.trim();
  if (!note) return;
  await api("/api/notes", {
    method: "POST",
    body: JSON.stringify({
      run_path: currentPath,
      record_key: selectedRecord.key,
      record_type: selectedRecord.sourceType,
      note,
      zones: Object.fromEntries(zoneIds.map((zone) => [zone, zones[zone].map((card) => card.name)])),
    }),
  });
  manualLog.push("note saved");
  els.noteText.value = "";
  renderManualLog();
}

function renderManualLog() {
  els.manualLog.innerHTML = "";
  for (const item of manualLog.slice(-80)) {
    const li = document.createElement("li");
    li.textContent = item;
    els.manualLog.append(li);
  }
}

async function loadLocalFile(event) {
  const file = event.target.files?.[0];
  if (!file) return;
  const text = await file.text();
  const payload = JSON.parse(text);
  setCurrentRun(payload, file.name);
}

function formatPct(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return "0%";
  return `${(Number(value) * 100).toFixed(2)}%`;
}

function formatSignedPct(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return "-";
  const number = Number(value) * 100;
  return `${number >= 0 ? "+" : ""}${number.toFixed(2)}%`;
}

function formatNumber(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return "-";
  return Number(value).toFixed(4);
}

function formatSignedNumber(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return "-";
  const number = Number(value);
  return `${number >= 0 ? "+" : ""}${number.toFixed(4)}`;
}

function deltaClass(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number === 0) return "";
  return number > 0 ? "positive" : "negative";
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}
