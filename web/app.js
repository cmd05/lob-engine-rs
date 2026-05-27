const scenarios = {
  replay: {
    endpoint: "/api/replay/stream",
    eyebrow: "Experiment 01",
    title: "Deterministic replay",
    copy: "The backend streams LOBSTER message rows and validated book snapshots in order.",
    visualTitle: "Order book reconstruction",
    chartLabel: "top-of-book quantity",
  },
  matching: {
    endpoint: "/api/matching/stream",
    eyebrow: "Experiment 02",
    title: "FIFO matching",
    copy: "A synthetic aggressive order consumes resting liquidity in strict price-time priority.",
    visualTitle: "FIFO queue consumption",
    chartLabel: "filled quantity by step",
  },
  latency: {
    endpoint: "/api/latency/stream",
    eyebrow: "Experiment 03",
    title: "Latency race",
    copy: "Orders are scheduled by arrival timestamp, so microseconds decide who obtains queue priority.",
    visualTitle: "Co-located versus remote arrival",
    chartLabel: "arrival latency",
  },
  strategy: {
    endpoint: "/api/strategy/stream",
    eyebrow: "Experiment 04",
    title: "Strategy behavior",
    copy: "Execution mechanics propagate into PnL, fill ratio, inventory, and adverse selection.",
    visualTitle: "Strategy equity evolution",
    chartLabel: "equity curve",
  },
};

let activeScenario = "replay";
let source = null;
let chart = null;
let eventCount = 0;

const $ = (id) => document.getElementById(id);

const els = {
  status: $("backendStatus"),
  statusWrap: document.querySelector(".status"),
  metricEvents: $("metricEvents"),
  metricValidation: $("metricValidation"),
  metricRace: $("metricRace"),
  metricPnl: $("metricPnl"),
  scenarioEyebrow: $("scenarioEyebrow"),
  scenarioTitle: $("scenarioTitle"),
  scenarioCopy: $("scenarioCopy"),
  visualTitle: $("visualTitle"),
  chartLabel: $("chartLabel"),
  narrative: $("narrative"),
  visual: $("visual"),
  tape: $("tape"),
  streamClock: $("streamClock"),
  eventCount: $("eventCount"),
  chartFallback: $("chartFallback"),
};

function formatInt(value) {
  return new Intl.NumberFormat("en-US").format(value);
}

function resetCounters() {
  eventCount = 0;
  els.eventCount.textContent = "0 events";
  els.tape.innerHTML = "";
  els.streamClock.textContent = "idle";
}

function setScenario(name) {
  stopStream();
  activeScenario = name;
  const scenario = scenarios[name];
  document.querySelectorAll(".step").forEach((button) => {
    button.classList.toggle("active", button.dataset.scenario === name);
  });
  els.scenarioEyebrow.textContent = scenario.eyebrow;
  els.scenarioTitle.textContent = scenario.title;
  els.scenarioCopy.textContent = scenario.copy;
  els.visualTitle.textContent = scenario.visualTitle;
  els.chartLabel.textContent = scenario.chartLabel;
  resetCounters();
  resetVisual();
  resetChart();
  els.narrative.textContent = "Press play to stream this experiment from the Rust backend.";
}

async function loadSummary() {
  try {
    const response = await fetch("/api/summary");
    const data = await response.json();
    els.status.textContent = data.message || "online";
    els.statusWrap.classList.add("online");
    els.metricEvents.textContent = formatInt(data.events);
    els.metricValidation.textContent = data.validation;
    els.metricRace.textContent = `${data.colocatedWins}:${data.remoteWins}`;
    els.metricPnl.textContent = `$${Number(data.marketMakerPnl).toFixed(2)}`;
  } catch {
    els.status.textContent = "backend offline";
  }
}

function resetVisual() {
  if (activeScenario === "replay") {
    els.visual.className = "visual replayView";
    els.visual.innerHTML = `
      <div class="ladder" id="askLadder"></div>
      <div class="midline"><span>ASK</span><span>validated book</span><span>BID</span></div>
      <div class="ladder" id="bidLadder"></div>
    `;
    renderLadder([], []);
  } else if (activeScenario === "matching") {
    els.visual.className = "visual queueView";
    els.visual.innerHTML = `<div class="queueRail" id="queueRail"></div><div id="fillText">Waiting for queue events.</div>`;
  } else if (activeScenario === "latency") {
    els.visual.className = "visual raceView";
    els.visual.innerHTML = `
      <div class="raceTrack">
        <div class="finish"></div>
        <div class="lane"><span>colocated market maker</span><i class="runner fast" id="runnerColocated"></i></div>
        <div class="lane"><span>remote trader</span><i class="runner slow" id="runnerRemote"></i></div>
        <div class="lane"><span>latency arbitrage trader</span><i class="runner" id="runnerArb"></i></div>
      </div>
      <div id="raceText">Waiting for arrivals.</div>
    `;
  } else {
    els.visual.className = "visual strategyView";
    els.visual.innerHTML = `
      <div class="strategyStats">
        <div><span>Market maker</span><strong id="mmStat">0</strong></div>
        <div><span>Order flow imbalance</span><strong id="ofiStat">0</strong></div>
        <div><span>Latency arbitrage</span><strong id="arbStat">0</strong></div>
      </div>
      <div id="strategyText">Waiting for strategy marks.</div>
    `;
  }
}

function resetChart() {
  if (chart) {
    chart.destroy();
    chart = null;
  }
  if (!window.Chart) {
    els.chartFallback.style.display = "block";
    return;
  }
  els.chartFallback.style.display = "none";
  const ctx = $("liveChart");
  const color = activeScenario === "replay" ? "#00e58a" : activeScenario === "latency" ? "#4da3ff" : "#fafafa";
  chart = new Chart(ctx, {
    type: activeScenario === "matching" ? "bar" : "line",
    data: {
      labels: [],
      datasets: [{ label: scenarios[activeScenario].chartLabel, data: [], borderColor: color, backgroundColor: color, tension: 0.25 }],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false,
      plugins: { legend: { labels: { color: "#bdbdbd" } } },
      scales: {
        x: { ticks: { color: "#8a8a8a", maxTicksLimit: 8 }, grid: { color: "#1f1f1f" } },
        y: { ticks: { color: "#8a8a8a" }, grid: { color: "#1f1f1f" } },
      },
    },
  });
}

function pushChart(label, value) {
  if (!chart) return;
  chart.data.labels.push(label);
  chart.data.datasets[0].data.push(value);
  if (chart.data.labels.length > 48) {
    chart.data.labels.shift();
    chart.data.datasets[0].data.shift();
  }
  chart.update();
}

function addTape(time, kind, explanation) {
  eventCount += 1;
  els.eventCount.textContent = `${eventCount} events`;
  const row = document.createElement("div");
  row.className = "tapeRow";
  row.innerHTML = `<span>${time}</span><b>${kind}</b><div>${explanation}</div>`;
  els.tape.prepend(row);
}

function playStream() {
  stopStream();
  resetCounters();
  resetVisual();
  resetChart();
  const scenario = scenarios[activeScenario];
  els.streamClock.textContent = "streaming";
  els.narrative.textContent = "Opening server-sent event stream...";
  source = new EventSource(scenario.endpoint);
  source.onmessage = (event) => {
    const data = JSON.parse(event.data);
    if (data.type === "done") {
      els.streamClock.textContent = "complete";
      els.narrative.textContent = "Stream complete. The same run is reproducible because the backend is deterministic.";
      stopStream(false);
      return;
    }
    routeEvent(data);
  };
  source.onerror = () => {
    els.streamClock.textContent = "stream error";
    els.narrative.textContent = "The SSE connection closed or the backend is not running.";
    stopStream(false);
  };
}

function stopStream(setIdle = true) {
  if (source) {
    source.close();
    source = null;
  }
  if (setIdle) els.streamClock.textContent = "idle";
}

function routeEvent(data) {
  if (data.type === "replay") handleReplay(data);
  if (data.type === "matching") handleMatching(data);
  if (data.type === "latency") handleLatency(data);
  if (data.type === "strategy") handleStrategy(data);
}

function handleReplay(data) {
  els.narrative.textContent = `${data.eventKind}: ${data.explanation}. The visible book is compared against the LOBSTER snapshot at this event index.`;
  els.streamClock.textContent = `event ${data.index}`;
  renderLadder(data.asks, data.bids);
  addTape(String(data.index), data.eventKind, data.explanation);
  const bestAsk = data.asks[0]?.qty || 0;
  const bestBid = data.bids[0]?.qty || 0;
  pushChart(data.index, bestAsk + bestBid);
}

function renderLadder(asks, bids) {
  const askLadder = $("askLadder");
  const bidLadder = $("bidLadder");
  if (!askLadder || !bidLadder) return;
  askLadder.innerHTML = (asks.length ? asks : Array.from({ length: 5 }, () => ({ price: "-", qty: "-" })))
    .map((level) => `<div class="level ask"><span class="price">${level.price}</span><span class="qty">${level.qty}</span></div>`)
    .join("");
  bidLadder.innerHTML = (bids.length ? bids : Array.from({ length: 5 }, () => ({ price: "-", qty: "-" })))
    .map((level) => `<div class="level bid"><span class="price">${level.price}</span><span class="qty">${level.qty}</span></div>`)
    .join("");
}

function handleMatching(data) {
  els.narrative.textContent = `${data.cause}. Result: ${data.effect}.`;
  els.streamClock.textContent = data.time;
  const rail = $("queueRail");
  rail.innerHTML = data.queue.map((item) => {
    const state = String(item.state).replace(" ", "-");
    return `<div class="queueBlock ${state}"><b>${item.id}</b><br><span>${item.qty} shares</span><br><small>${item.state}</small></div>`;
  }).join("");
  const filled = data.fills.reduce((sum, fill) => sum + fill.qty, 0);
  $("fillText").textContent = filled ? `Filled ${filled} shares in FIFO order.` : "No executions yet; resting orders keep priority.";
  addTape(data.time, `step ${data.step}`, data.effect);
  pushChart(data.step, filled);
}

function handleLatency(data) {
  els.narrative.textContent = `${data.agent} arrived after ${Math.round(data.latencyNs / 1000)} us and received ${data.status}.`;
  els.streamClock.textContent = `arrival ${data.seq}`;
  const id = data.agent.includes("colocated") ? "runnerColocated" : data.agent.includes("remote") ? "runnerRemote" : "runnerArb";
  const runner = $(id);
  const scaled = Math.min(94, Math.max(6, 100 - data.latencyNs / 500000));
  if (runner) runner.style.left = `${scaled}%`;
  $("raceText").textContent = `Order ${data.orderId}: ${data.filledQty} filled at arrival timestamp ${data.arrivalTs}.`;
  addTape(String(data.seq), data.agent, `${data.status}, ${data.filledQty} filled`);
  pushChart(data.seq, Math.round(data.latencyNs / 1000));
}

function handleStrategy(data) {
  els.narrative.textContent = `Strategy marks updated. Execution quality is tracked as equity, inventory, fill behavior, and adverse selection.`;
  els.streamClock.textContent = `mark ${data.step}`;
  $("mmStat").textContent = data.marketMaker;
  $("ofiStat").textContent = data.ofi;
  $("arbStat").textContent = data.latencyArb;
  $("strategyText").textContent = `Market-maker inventory ${data.inventory}, final PnL $${Number(data.pnlDollars).toFixed(2)}.`;
  addTape(String(data.step), "strategy mark", `MM ${data.marketMaker}, OFI ${data.ofi}, arb ${data.latencyArb}`);
  pushChart(data.step, data.marketMaker);
}

document.querySelectorAll(".step").forEach((button) => {
  button.addEventListener("click", () => setScenario(button.dataset.scenario));
});

$("playBtn").addEventListener("click", playStream);
$("stopBtn").addEventListener("click", () => stopStream());
$("resetBtn").addEventListener("click", () => setScenario(activeScenario));

setScenario("replay");
loadSummary();
